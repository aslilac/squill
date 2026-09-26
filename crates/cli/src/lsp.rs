//! `squill lsp`: a language server, for editors that speak LSP.
//!
//! It offers whole-document formatting and publishes squill's
//! diagnostics (statements passed through verbatim, embedded strings it
//! declined) as warnings. A document resolves against its path exactly
//! as `squill fmt --stdin-filepath` does: config, rules, `ignore`, and
//! `frozen` all apply, and a host file's embedded SQL formats too.
//! Config is re-read for every request, so editing `squill.toml` takes
//! effect without a restart; loaded wasm grammars stay cached.

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
use lsp_types::DiagnosticSeverity;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::DocumentFormattingParams;
use lsp_types::InitializeParams;
use lsp_types::OneOf;
use lsp_types::Position;
use lsp_types::PositionEncodingKind;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::Range;
use lsp_types::ServerCapabilities;
use lsp_types::ServerInfo;
use lsp_types::TextDocumentSyncCapability;
use lsp_types::TextDocumentSyncKind;
use lsp_types::TextEdit;
use lsp_types::Uri;
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;

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
		eprintln!("squill lsp: {message}");
		return ExitCode::from(1);
	}
	match io_threads.join() {
		Ok(()) => ExitCode::SUCCESS,
		Err(err) => {
			eprintln!("squill lsp: {err}");
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
	let capabilities = ServerCapabilities {
		position_encoding: Some(if utf8 {
			PositionEncodingKind::UTF8
		} else {
			PositionEncodingKind::UTF16
		}),
		text_document_sync: Some(TextDocumentSyncCapability::Kind(
			TextDocumentSyncKind::FULL,
		)),
		document_formatting_provider: Some(OneOf::Left(true)),
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
	server.main_loop()
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

	/// Resolve a document the way `--stdin-filepath` would. `None` when
	/// squill leaves it alone. Config is re-read every time.
	fn resolve(&mut self, uri: &Uri) -> Result<Option<Resolved>, String> {
		self.caches.forget_config();
		let Some(document) = self.documents.get(uri) else {
			return Ok(None);
		};
		match file_path(uri) {
			Some(path) => {
				crate::stdin_as(&path, &self.args, &self.cwd, &mut self.caches)
			}
			// An unsaved, untitled buffer has no path to resolve rules
			// against: plain SQL on the workspace's top-level config.
			None if document.language_id == "sql" => {
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
}
