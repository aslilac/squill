# Fuzzing

Two libFuzzer targets guard the lossless-CST invariants on arbitrary input, in both dialects:

- **`lex_roundtrip`** — lexing never panics, and the token texts concatenate back to the input byte-for-byte.
- **`parse_roundtrip`** — parsing never panics, and the CST reproduces the input byte-for-byte.

## Setup

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

## Seeding the corpus

libFuzzer mutates whatever is in `corpus/<target>/`; starting from valid SQL reaches deep parser states far faster than starting from random bytes. Seed both targets from the checked-in SQL corpus (small files mutate best, so cap the size):

```sh
cd crates/parser/fuzz
mkdir -p corpus/lex_roundtrip corpus/parse_roundtrip
for f in ../../../corpus/coder/*/*.sql; do
  if [ "$(stat -c%s "$f")" -lt 4096 ]; then
    h=$(sha1sum "$f" | cut -c1-16)
    cp "$f" "corpus/lex_roundtrip/$h"
    cp "$f" "corpus/parse_roundtrip/$h"
  fi
done
```

The corpus directories are gitignored: they are regenerable from the SQL corpus, and the fuzzer grows them with its own discoveries as it runs.

## Running

```sh
cd crates/parser/fuzz
cargo +nightly fuzz run lex_roundtrip -- -max_total_time=600 -max_len=4096
cargo +nightly fuzz run parse_roundtrip -- -max_total_time=600 -max_len=4096
```

Drop `-max_total_time` to fuzz until interrupted. On a crash, libFuzzer writes the failing input to `artifacts/<target>/`; reproduce with:

```sh
cargo +nightly fuzz run <target> artifacts/<target>/<file>
```

and shrink it first with `cargo +nightly fuzz tmin <target> <file>`.
