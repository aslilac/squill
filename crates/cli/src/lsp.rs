//! `squill language-server start`: a language server, for editors that
//! speak LSP.
//!
//! It offers whole-document formatting, publishes squill's diagnostics
//! (statements passed through verbatim, embedded strings it declined) as
//! warnings, and highlights the SQL embedded in a host file (every
//! string the rule's query finds) as semantic tokens; editors highlight
//! SQL files well themselves. A document resolves against its path exactly
//! as `squill fmt --stdin-filepath` does: config, rules, `ignore`, and
//! `frozen` all apply, and a host file's embedded SQL formats too.
//! Config is re-read for every request, so editing `squill.toml` takes
//! effect without a restart; loaded wasm grammars stay cached.
//!
//! Two affordances for editors that run one formatter per file (VS
//! Code): a `source.formatSql` code action applies the same edit, so it
//! can run on save after another language's formatter; and a client can
//! pass `{"formattingSelector": [...document filters]}` as initialization
//! options, and squill registers as a formatter for just those documents
//! (dynamically, when the client supports it) instead of for all.
//! `semanticTokensSelector` does the same for highlighting, for editors
//! that take one server's semantic tokens per document and would lose
//! the host language server's; with `[]`, a client can still ask for
//! tokens itself and draw them its own way.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use lsp_server::Connection;
use lsp_server::ErrorCode;
use lsp_server::Message;
use lsp_server::Notification;
use lsp_server::Request;
use lsp_server::Response;
use lsp_types::CodeAction;
use lsp_types::CodeActionKind;
use lsp_types::CodeActionOptions;
use lsp_types::CodeActionOrCommand;
use lsp_types::CodeActionParams;
use lsp_types::CodeActionProviderCapability;
use lsp_types::DiagnosticSeverity;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::DocumentFormattingParams;
use lsp_types::DocumentSelector;
use lsp_types::InitializeParams;
use lsp_types::OneOf;
use lsp_types::Position;
use lsp_types::PositionEncodingKind;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::Range;
use lsp_types::Registration;
use lsp_types::RegistrationParams;
use lsp_types::SemanticToken;
use lsp_types::SemanticTokenType;
use lsp_types::SemanticTokens;
use lsp_types::SemanticTokensFullOptions;
use lsp_types::SemanticTokensLegend;
use lsp_types::SemanticTokensOptions;
use lsp_types::SemanticTokensParams;
use lsp_types::SemanticTokensRegistrationOptions;
use lsp_types::ServerCapabilities;
use lsp_types::ServerInfo;
use lsp_types::StaticRegistrationOptions;
use lsp_types::TextDocumentRegistrationOptions;
use lsp_types::TextDocumentSyncCapability;
use lsp_types::TextDocumentSyncKind;
use lsp_types::TextEdit;
use lsp_types::Uri;
use lsp_types::WorkspaceEdit;
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;

use formatter::highlight::HighlightKind;
use formatter::highlight::highlight;

use crate::Args;
use crate::Caches;
use crate::Resolved;

/// An open document, as the client last sent it.
struct Document {
	text: String,
	language_id: String,
	version: i32,
}

struct Server {
	connection: Connection,
	documents: HashMap<Uri, Document>,
	caches: Caches,
	args: Args,
	cwd: PathBuf,
	/// Positions in UTF-8 bytes when the client allows it, else the
	/// protocol's default UTF-16 code units.
	utf8: bool,
}

pub fn run() -> ExitCode {
	let (connection, io_threads) = Connection::stdio();
	let result = serve(connection);
	if let Err(message) = result {
		eprintln!("squill language-server: {message}");
		return ExitCode::from(1);
	}
	match io_threads.join() {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			eprintln!("squill language-server: {err}");
			ExitCode::from(1)
		}
	}
}

fn serve(connection: Connection) -> Result<(), String> {
	let (id, params) =
		connection.initialize_start().map_err(|err| err.to_string())?;
	let params: InitializeParams =
		serde_json::from_value(params).map_err(|err| err.to_string())?;
	let utf8 = params
		.capabilities
		.general
		.as_ref()
		.and_then(|general| general.position_encodings.as_ref())
		.is_some_and(|encodings| encodings.contains(&PositionEncodingKind::UTF8));
	// Formatting for only the documents the client asked about, when it
	// asked and can take a dynamic registration; everywhere otherwise.
	let formatting_selector = params
		.initialization_options
		.as_ref()
		.and_then(|options| options.get("formattingSelector"))
		.filter(|selector| selector.is_array())
		.cloned()
		.filter(|_| {
			params
				.capabilities
				.text_document
				.as_ref()
				.and_then(|text| text.formatting.as_ref())
				.and_then(|formatting| formatting.dynamic_registration)
				.unwrap_or(false)
		});
	// Highlighting for only the documents the client asked about. A
	// static registration can carry its own selector, so this needs
	// nothing dynamic.
	let semantic_tokens_selector: Option<DocumentSelector> = params
		.initialization_options
		.as_ref()
		.and_then(|options| options.get("semanticTokensSelector"))
		.and_then(|selector| serde_json::from_value(selector.clone()).ok());
	let semantic_tokens = SemanticTokensOptions {
		legend: legend(),
		full: Some(SemanticTokensFullOptions::Bool(true)),
		..SemanticTokensOptions::default()
	};
	let capabilities = ServerCapabilities {
		position_encoding: Some(if utf8 {
			PositionEncodingKind::UTF8
		} else {
			PositionEncodingKind::UTF16
		}),
		text_document_sync: Some(TextDocumentSyncCapability::Kind(
			TextDocumentSyncKind::FULL,
		)),
		document_formatting_provider: formatting_selector
			.is_none()
			.then_some(OneOf::Left(true)),
		code_action_provider: Some(CodeActionProviderCapability::Options(
			CodeActionOptions {
				code_action_kinds: Some(vec![format_sql_kind()]),
				..CodeActionOptions::default()
			},
		)),
		semantic_tokens_provider: Some(match semantic_tokens_selector {
			None => semantic_tokens.into(),
			Some(selector) => SemanticTokensRegistrationOptions {
				text_document_registration_options: TextDocumentRegistrationOptions {
					document_selector: Some(selector),
				},
				semantic_tokens_options: semantic_tokens,
				static_registration_options: StaticRegistrationOptions::default(),
			}
			.into(),
		}),
		..ServerCapabilities::default()
	};
	let result = serde_json::json!({
		"capabilities": capabilities,
		"serverInfo": ServerInfo {
			name: "squill".to_string(),
			version: Some(env!("CARGO_PKG_VERSION").to_string()),
		},
	});
	connection.initialize_finish(id, result).map_err(|err| err.to_string())?;

	let mut server = Server {
		connection,
		documents: HashMap::new(),
		caches: Caches::default(),
		args: Args::default(),
		cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
		utf8,
	};
	if let Some(selector) = formatting_selector {
		let registration = Registration {
			id: "squill-formatting".to_string(),
			method: lsp_types::request::Formatting::METHOD.to_string(),
			register_options: Some(
				serde_json::json!({ "documentSelector": selector }),
			),
		};
		server.send(
			Request::new(
				"squill-register-formatting".to_string().into(),
				lsp_types::request::RegisterCapability::METHOD.to_string(),
				RegistrationParams { registrations: vec![registration] },
			)
			.into(),
		)?;
	}
	server.main_loop()
}

/// Each highlight kind's semantic token type. A token's type is its
/// index here, and the legend lists them in this order.
const TOKEN_TYPES: [(HighlightKind, SemanticTokenType); 9] = [
	(HighlightKind::Keyword, SemanticTokenType::KEYWORD),
	(HighlightKind::Name, SemanticTokenType::VARIABLE),
	(HighlightKind::Function, SemanticTokenType::FUNCTION),
	(HighlightKind::Type, SemanticTokenType::TYPE),
	(HighlightKind::String, SemanticTokenType::STRING),
	(HighlightKind::Number, SemanticTokenType::NUMBER),
	(HighlightKind::Parameter, SemanticTokenType::PARAMETER),
	(HighlightKind::Operator, SemanticTokenType::OPERATOR),
	(HighlightKind::Comment, SemanticTokenType::COMMENT),
];

fn legend() -> SemanticTokensLegend {
	SemanticTokensLegend {
		token_types: TOKEN_TYPES.into_iter().map(|(_, name)| name).collect(),
		token_modifiers: Vec::new(),
	}
}

fn token_type(kind: HighlightKind) -> u32 {
	let index = TOKEN_TYPES.iter().position(|(listed, _)| *listed == kind);
	index.expect("every highlight kind is in the legend") as u32
}

/// `source.formatSql`: squill's formatting, as a code action.
fn format_sql_kind() -> CodeActionKind {
	CodeActionKind::new("source.formatSql")
}

impl Server {
	fn main_loop(&mut self) -> Result<(), String> {
		while let Ok(message) = self.connection.receiver.recv() {
			match message {
				Message::Request(request) => {
					if self
						.connection
						.handle_shutdown(&request)
						.map_err(|err| err.to_string())?
					{
						return Ok(());
					}
					self.request(request)?;
				}
				Message::Notification(notification) => {
					self.notification(notification)?
				}
				Message::Response(_) => {}
			}
		}
		Ok(())
	}

	fn send(&self, message: Message) -> Result<(), String> {
		self.connection.sender.send(message).map_err(|err| err.to_string())
	}

	fn request(&mut self, request: Request) -> Result<(), String> {
		let response = match request.method.as_str() {
			lsp_types::request::Formatting::METHOD => {
				match serde_json::from_value::<DocumentFormattingParams>(request.params)
				{
					Ok(params) => match self.format(&params.text_document.uri) {
						Ok(edits) => Response::new_ok(request.id, edits),
						Err(message) => Response::new_err(
							request.id,
							ErrorCode::RequestFailed as i32,
							message,
						),
					},
					Err(err) => Response::new_err(
						request.id,
						ErrorCode::InvalidParams as i32,
						err.to_string(),
					),
				}
			}
			lsp_types::request::CodeActionRequest::METHOD => {
				match serde_json::from_value::<CodeActionParams>(request.params) {
					Ok(params) => {
						let actions = self.code_actions(&params);
						Response::new_ok(request.id, actions)
					}
					Err(err) => Response::new_err(
						request.id,
						ErrorCode::InvalidParams as i32,
						err.to_string(),
					),
				}
			}
			lsp_types::request::SemanticTokensFullRequest::METHOD => {
				match serde_json::from_value::<SemanticTokensParams>(request.params) {
					Ok(params) => {
						let tokens = self.semantic_tokens(&params.text_document.uri);
						Response::new_ok(request.id, tokens)
					}
					Err(err) => Response::new_err(
						request.id,
						ErrorCode::InvalidParams as i32,
						err.to_string(),
					),
				}
			}
			_ => Response::new_err(
				request.id,
				ErrorCode::MethodNotFound as i32,
				format!("squill does not handle `{}`", request.method),
			),
		};
		self.send(response.into())
	}

	fn notification(&mut self, notification: Notification) -> Result<(), String> {
		match notification.method.as_str() {
			lsp_types::notification::DidOpenTextDocument::METHOD => {
				let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(
					notification.params,
				) else {
					return Ok(());
				};
				let document = params.text_document;
				self.documents.insert(
					document.uri.clone(),
					Document {
						text: document.text,
						language_id: document.language_id,
						version: document.version,
					},
				);
				self.publish_diagnostics(&document.uri)
			}
			lsp_types::notification::DidChangeTextDocument::METHOD => {
				let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(
					notification.params,
				) else {
					return Ok(());
				};
				let uri = params.text_document.uri;
				// Full sync: the last change is the whole document.
				if let (Some(document), Some(change)) = (
					self.documents.get_mut(&uri),
					params.content_changes.into_iter().last(),
				) {
					document.text = change.text;
					document.version = params.text_document.version;
				}
				self.publish_diagnostics(&uri)
			}
			lsp_types::notification::DidCloseTextDocument::METHOD => {
				let Ok(params) = serde_json::from_value::<DidCloseTextDocumentParams>(
					notification.params,
				) else {
					return Ok(());
				};
				let uri = params.text_document.uri;
				self.documents.remove(&uri);
				self.send_diagnostics(uri, Vec::new(), None)
			}
			_ => Ok(()),
		}
	}

	/// Resolve a document the way `--stdin-filepath` would, except that
	/// only a document the client calls SQL can be SQL for want of any
	/// rule saying so. `None` when squill leaves it alone. Config is
	/// re-read every time.
	fn resolve(&mut self, uri: &Uri) -> Result<Option<Resolved>, String> {
		self.caches.forget_config();
		let Some(document) = self.documents.get(uri) else {
			return Ok(None);
		};
		let is_sql = document.language_id == "sql";
		match file_path(uri) {
			Some(path) => {
				crate::stdin_as(&path, &self.args, &self.cwd, &mut self.caches, is_sql)
			}
			// An unsaved, untitled buffer has no path to resolve rules
			// against: plain SQL on the workspace's top-level config.
			None if is_sql => {
				crate::resolve_sql_defaults(&self.cwd, &self.args, &mut self.caches)
					.map(Some)
			}
			None => Ok(None),
		}
	}

	fn format(&mut self, uri: &Uri) -> Result<Vec<TextEdit>, String> {
		let Some(resolved) = self.resolve(uri)? else {
			return Ok(Vec::new());
		};
		let Some(document) = self.documents.get(uri) else {
			return Ok(Vec::new());
		};
		let outcome = crate::format_resolved(&document.text, &resolved)?;
		if outcome.formatted == document.text {
			return Ok(Vec::new());
		}
		let whole = Range {
			start: Position::new(0, 0),
			end: self.position(&document.text, document.text.len()),
		};
		Ok(vec![TextEdit::new(whole, outcome.formatted)])
	}

	/// "Format SQL with squill", when it would change something and the
	/// client wants source actions. A failure is no action, not an error:
	/// code actions are asked for constantly, and the diagnostics already
	/// say what went wrong.
	fn code_actions(
		&mut self,
		params: &CodeActionParams,
	) -> Vec<CodeActionOrCommand> {
		let wanted = params.context.only.as_ref().is_none_or(|only| {
			only.iter().any(|kind| {
				let kind = kind.as_str();
				kind == "source" || format_sql_kind().as_str().starts_with(kind)
			})
		});
		if !wanted {
			return Vec::new();
		}
		let uri = &params.text_document.uri;
		let Ok(edits) = self.format(uri) else {
			return Vec::new();
		};
		if edits.is_empty() {
			return Vec::new();
		}
		let action = CodeAction {
			title: "Format SQL with squill".to_string(),
			kind: Some(format_sql_kind()),
			edit: Some(WorkspaceEdit {
				changes: Some(HashMap::from([(uri.clone(), edits)])),
				..WorkspaceEdit::default()
			}),
			..CodeAction::default()
		};
		vec![CodeActionOrCommand::CodeAction(action)]
	}

	/// Where the keywords, names, literals and comments are in a host
	/// file's embedded SQL. `None` for a SQL file, which the editor
	/// highlights well itself. Like code actions, a failure is no tokens,
	/// not an error; the diagnostics say what went wrong.
	fn semantic_tokens(&mut self, uri: &Uri) -> Option<SemanticTokens> {
		let resolved = self.resolve(uri).ok()??;
		if matches!(resolved.kind, crate::Kind::Sql) {
			return None;
		}
		let document = self.documents.get(uri)?;
		let located = crate::locate_resolved(&document.text, &resolved).ok()?;
		let mut spans = Vec::new();
		for sql in located {
			let options =
				formatter::Options { dialect: sql.dialect, ..resolved.options };
			let offset = sql.range.start;
			let highlights = highlight(&document.text[sql.range], &options);
			spans.extend(highlights.into_iter().map(|highlight| {
				let range =
					highlight.range.start + offset..highlight.range.end + offset;
				(range, token_type(highlight.kind))
			}));
		}
		Some(SemanticTokens {
			result_id: None,
			data: encode_tokens(&document.text, &spans, self.utf8),
		})
	}

	fn publish_diagnostics(&mut self, uri: &Uri) -> Result<(), String> {
		let diagnostics = match self.resolve(uri) {
			Ok(Some(resolved)) => {
				let Some(document) = self.documents.get(uri) else {
					return Ok(());
				};
				match crate::format_resolved(&document.text, &resolved) {
					Ok(outcome) => outcome
						.diagnostics
						.iter()
						.map(|diagnostic| {
							let range = diagnostic.range.clone().unwrap_or(0..0);
							self.diagnostic(
								&document.text,
								range,
								DiagnosticSeverity::WARNING,
								&diagnostic.message,
							)
						})
						.collect(),
					Err(message) => vec![self.diagnostic(
						&document.text,
						0..0,
						DiagnosticSeverity::ERROR,
						&message,
					)],
				}
			}
			Ok(None) => Vec::new(),
			// A broken squill.toml: say so where the user is looking.
			Err(message) => {
				let text =
					self.documents.get(uri).map_or("", |document| &document.text);
				vec![self.diagnostic(text, 0..0, DiagnosticSeverity::ERROR, &message)]
			}
		};
		let version = self.documents.get(uri).map(|document| document.version);
		self.send_diagnostics(uri.clone(), diagnostics, version)
	}

	fn send_diagnostics(
		&self,
		uri: Uri,
		diagnostics: Vec<lsp_types::Diagnostic>,
		version: Option<i32>,
	) -> Result<(), String> {
		let params = PublishDiagnosticsParams { uri, diagnostics, version };
		self.send(
			Notification::new(
				lsp_types::notification::PublishDiagnostics::METHOD.to_string(),
				params,
			)
			.into(),
		)
	}

	fn diagnostic(
		&self,
		text: &str,
		range: std::ops::Range<usize>,
		severity: DiagnosticSeverity,
		message: &str,
	) -> lsp_types::Diagnostic {
		lsp_types::Diagnostic {
			range: Range {
				start: self.position(text, range.start),
				end: self.position(text, range.end),
			},
			severity: Some(severity),
			source: Some("squill".to_string()),
			message: message.to_string(),
			..lsp_types::Diagnostic::default()
		}
	}

	/// The LSP position of a byte offset, in the negotiated encoding.
	fn position(&self, text: &str, offset: usize) -> Position {
		position(text, offset, self.utf8)
	}
}

/// The position of byte `offset` in `text`: a line, and a column in
/// UTF-8 bytes or UTF-16 code units.
fn position(text: &str, offset: usize, utf8: bool) -> Position {
	let offset = offset.min(text.len());
	let before = &text[..offset];
	let line = before.matches('\n').count();
	let line_start = before.rfind('\n').map_or(0, |at| at + 1);
	let column = &text[line_start..offset];
	let character =
		if utf8 { column.len() } else { column.chars().map(char::len_utf16).sum() };
	Position::new(line as u32, character as u32)
}

/// LSP's encoding of `spans`, byte ranges into `text` in order, each
/// with its token type: every token placed relative to the one before,
/// in the negotiated position encoding. A span over several lines is a
/// token per line, since clients needn't take tokens that cross lines. A
/// span overlapping the one before it (a string captured twice, by
/// nested patterns) is dropped.
fn encode_tokens(
	text: &str,
	spans: &[(std::ops::Range<usize>, u32)],
	utf8: bool,
) -> Vec<SemanticToken> {
	let width = |text: &str| {
		let units =
			if utf8 { text.len() } else { text.chars().map(char::len_utf16).sum() };
		units as u32
	};
	let mut tokens = Vec::new();
	// Lines are counted up to `scanned`; `line` starts at `line_start`.
	let (mut scanned, mut line, mut line_start) = (0, 0, 0);
	// Where the last token was, which the next is relative to.
	let (mut last_line, mut last_column) = (0, 0);
	let mut covered = 0;
	for (range, token_type) in spans {
		if range.start < covered {
			continue;
		}
		covered = range.end;
		let mut start = range.start;
		while start < range.end {
			for (at, byte) in text[scanned..start].bytes().enumerate() {
				if byte == b'\n' {
					line += 1;
					line_start = scanned + at + 1;
				}
			}
			scanned = start;
			let end =
				text[start..range.end].find('\n').map_or(range.end, |at| start + at);
			let piece = text[start..end].trim_end_matches('\r');
			if !piece.is_empty() {
				let column = width(&text[line_start..start]);
				let delta_start =
					if line == last_line { column - last_column } else { column };
				tokens.push(SemanticToken {
					delta_line: line - last_line,
					delta_start,
					length: width(piece),
					token_type: *token_type,
					token_modifiers_bitset: 0,
				});
				(last_line, last_column) = (line, column);
			}
			// Past the newline, onto the span's next line.
			start = end + 1;
		}
	}
	tokens
}

/// The local path a `file:` URI names; `None` for any other scheme.
fn file_path(uri: &Uri) -> Option<PathBuf> {
	let rest = uri.as_str().strip_prefix("file://")?;
	// `file:///path`, or `file://localhost/path`.
	let rest = rest.strip_prefix("localhost").unwrap_or(rest);
	let decoded = percent_decode(rest)?;
	// Windows: `file:///C:/dir` names `C:/dir`.
	let bytes = decoded.as_bytes();
	let path = if bytes.len() >= 3
		&& bytes[0] == b'/'
		&& bytes[1].is_ascii_alphabetic()
		&& bytes[2] == b':'
	{
		&decoded[1..]
	} else {
		&decoded[..]
	};
	Some(Path::new(path).to_path_buf())
}

/// Decode `%XX` escapes; `None` for a malformed one or invalid UTF-8.
fn percent_decode(text: &str) -> Option<String> {
	let mut out = Vec::with_capacity(text.len());
	let mut bytes = text.bytes();
	while let Some(byte) = bytes.next() {
		if byte == b'%' {
			let high = (bytes.next()? as char).to_digit(16)?;
			let low = (bytes.next()? as char).to_digit(16)?;
			out.push((high * 16 + low) as u8);
		} else {
			out.push(byte);
		}
	}
	String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn file_uris_become_paths() {
		let path = |uri: &str| file_path(&uri.parse::<Uri>().expect("uri"));
		assert_eq!(
			path("file:///home/me/q.sql"),
			Some(PathBuf::from("/home/me/q.sql"))
		);
		assert_eq!(
			path("file:///home/me/my%20queries/q%C3%A9.sql"),
			Some(PathBuf::from("/home/me/my queries/qé.sql"))
		);
		assert_eq!(
			path("file://localhost/tmp/q.sql"),
			Some(PathBuf::from("/tmp/q.sql"))
		);
		assert_eq!(
			path("file:///C:/src/q.sql"),
			Some(PathBuf::from("C:/src/q.sql"))
		);
		assert_eq!(path("untitled:Untitled-1"), None);
	}

	#[test]
	fn positions_count_in_the_negotiated_encoding() {
		let text = "select 1;\nselect '\u{1f600}', x;";
		let x = text.find('x').expect("x");
		// Line 1; the emoji is 4 bytes in UTF-8, 2 units in UTF-16.
		assert_eq!(position(text, x, true), Position::new(1, 15));
		assert_eq!(position(text, x, false), Position::new(1, 13));
		assert_eq!(position(text, 0, false), Position::new(0, 0));
		assert_eq!(position(text, text.len(), false), Position::new(1, 15));
	}

	#[test]
	fn tokens_are_relative_and_split_at_lines() {
		// `(delta line, delta start, length, type)` for each token.
		let encode = |text: &str, spans: &[(std::ops::Range<usize>, u32)]| {
			encode_tokens(text, spans, false)
				.into_iter()
				.map(|token| {
					let SemanticToken { delta_line, delta_start, length, .. } = token;
					(delta_line, delta_start, length, token.token_type)
				})
				.collect::<Vec<_>>()
		};
		let text = "x = '\u{1f600}' /* a\r\nb */ y";
		let at = |word: &str| text.find(word).expect("word");
		let string = at("'")..at(" /*");
		let comment = at("/*")..at(" y");
		let y = at("y")..text.len();
		assert_eq!(
			encode(text, &[(0..1, 0), (string.clone(), 1), (comment, 2), (y, 0)]),
			[
				(0, 0, 1, 0),
				// The emoji is two UTF-16 units.
				(0, 4, 4, 1),
				// A comment over two lines is two tokens, without the `\r`.
				(0, 5, 4, 2),
				(1, 0, 4, 2),
				(0, 5, 1, 0),
			]
		);
		// A span overlapping the one before is dropped.
		assert_eq!(
			encode(text, &[(string.clone(), 1), (string.start + 1..string.end, 2)]),
			[(0, 4, 4, 1)]
		);
	}
}
