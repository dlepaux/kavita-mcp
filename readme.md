# kavita-mcp

An [MCP](https://modelcontextprotocol.io) server that lets each person in a household search their own
[Kavita](https://www.kavitareader.com) libraries from a chat, and send one book to their Kindle. It
acts as that person, with their own Kavita API key, and sends nothing until they confirm in the chat.

Every sentence a person reads is in their own language, chosen per person: English and French today,
and [another is one JSON file](#add-a-language).

## Tools

| Tool | What it does |
|------|--------------|
| `search_library` | Up to 10 books matching a title or series in the caller's libraries, each with a short handle, its library, format, size and whether it can go to a Kindle. Read-only. |
| `send_to_kindle` | Sends one book, named by a handle from the caller's own search, to the caller's Kindle registered in Kavita. It first asks the person to confirm (MCP form elicitation); a client that cannot ask sends nothing. |

The caller is whoever holds the bearer token. Their Kavita key, language and libraries come from
configuration, never from a tool argument.

## Routes

All on `KAVITA_MCP_ADDR`.

| Route | Auth | Purpose |
|-------|------|---------|
| `/mcp` | `Authorization: Bearer <token>` | Streamable HTTP MCP |
| `/metrics` | none | Prometheus counters |
| `/healthz` | none | Answers `ok`. The image is distroless, so it has no healthcheck of its own. |

`/metrics` and `/healthz` are unauthenticated: publish only `/mcp` beyond your private network.

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `KAVITA_URL` | required | Kavita's base URL, e.g. `http://kavita:5000` |
| `KAVITA_MCP_IDENTITIES` | required | Path to the identities file below |
| `KAVITA_MCP_KEY_<PROFILE>` | required, one per identity | That person's own Kavita API key, sent only as the `x-api-key` header. `<PROFILE>` is the profile upper-cased. Use a full API key, not an OPDS or an image-only key. |
| `KAVITA_MCP_ADMIN_NAME` | unset | The name people are told to contact when Kavita needs fixing ("Tell Bob."). Unset, the sentences say "your Kavita admin". At most 64 characters. |
| `KAVITA_SEND_SIZE_LIMIT_BYTES` | `26214400` | A book at or over this size is refused. Match Kavita's own email size limit. |
| `KAVITA_MCP_ADDR` | `0.0.0.0:8080` | Listen address |
| `RUST_LOG` | `info` | Log filter ([`tracing-subscriber` syntax](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)) |

The server refuses to start when a required variable is empty, the identities file is invalid, an
identity has no key, or a `KAVITA_MCP_KEY_*` variable has no identity.

The identities file, one entry per person (see [`examples/identities.yml`](examples/identities.yml)):

```yaml
- profile: alice          # [a-z0-9], 1 to 32 characters
  token_sha256: 10fe24a7…  # SHA-256 of Alice's bearer token, 64 hex characters
  language: fr            # a code from locales/: en, fr
  libraries: [Romans, Comics]   # Kavita library names, exactly as Kavita shows them
```

Make a token and its hash with `token=$(openssl rand -hex 32); printf %s "$token" | sha256sum`. The
token goes to that person's chat client; only the hash goes in the file.

Kavita must have email set up for devices, and each person must have their Kindle registered as a
device in their own Kavita account, with Kavita's sender address approved in their Amazon account.

## Running

Images for `linux/amd64` and `linux/arm64` are published to `ghcr.io/dlepaux/kavita-mcp`.

```bash
docker run -d --name kavita-mcp -p 8080:8080 \
  --env-file examples/kavita-mcp.env \
  -v "$PWD/examples/identities.yml:/config/identities.yml:ro" \
  ghcr.io/dlepaux/kavita-mcp:latest
```

[`examples/kavita-mcp.env`](examples/kavita-mcp.env) holds placeholders: replace every value. The
container runs as a non-root user (uid 65532), so the identities file must be readable by it.

Then point an MCP client that supports form elicitation at `http://<host>:8080/mcp` with the header
`Authorization: Bearer <that person's token>`.

## Security model

- **Bearer to identity.** A request to `/mcp` is served only when its bearer token's SHA-256 matches
  an identity, compared in constant time. Tokens are never stored in clear. A refused token is
  counted in `kavita_mcp_auth_failures_total`.
- **Each person acts as themselves.** Every Kavita call uses the caller's own key, in the `x-api-key`
  header and never in a URL, so Kavita applies that person's own permissions.
- **Only the person's libraries.** A search result outside the libraries configured for the caller
  is dropped (and counted in `kavita_mcp_search_skipped_total`), even when Kavita returns it.
  `send_to_kindle` accepts only a handle minted by the caller's own search in the last 60 minutes;
  a chapter id, a made-up handle or another person's handle is refused before Kavita is called. So
  kavita-mcp sends only books that appeared in that person's search of their configured libraries.
- **Nothing sent without a yes.** The format, size and Kavita's email setting are checked before
  asking; the send happens only after the person accepts, and not if the call was cancelled first.
- **One chapter per send**, never a whole series.
- **What it never logs or returns:** bearer tokens, Kavita keys and Kindle email addresses (the
  address is dropped while Kavita's answer is decoded, so it is never held). Kavita's own error text
  is never shown to the person; a short, control-stripped excerpt of it may be logged.
- **Book metadata is data.** Titles and names from book files are stripped of control characters
  and length-capped, and the server's instructions tell the model to treat them as data.

## Add a language

Everything a person reads is in [`locales/`](locales/), one JSON file per language. Text only the model
reads (tool descriptions), logs and metric names stay in English in the code.

1. Copy `locales/en.json` to `locales/<code>.json`, where the code is ISO 639-1.
2. Translate each value. Keep every `{placeholder}`: you may move it, but not rename or drop it.
   - `admin_named` and `no_kindle_named` are used when `KAVITA_MCP_ADMIN_NAME` is set, the
     `_unnamed` keys when it is not.
   - `{tell}` is filled with the `admin_named` or `admin_unnamed` sentence.
   - `decimal_mark` and `size` write a file size, such as `2.1 MB`.
3. Add one line for the file to `LOCALES` in `src/locale.rs`.
4. Run `cargo test`. The completeness test lists every missing or extra key, changed placeholder and
   empty value.

There is no fallback to English: a translation the test accepts is complete. An identity whose
`language` has no file refuses to start.

## Metrics

Every counter carries a `kind` label, and every kind exists at zero from start-up, so an alert can
read the first increase.

| Counter | Kinds |
|---------|-------|
| `kavita_mcp_kavita_failures_total` | `unreachable`, `http_5xx`, `key_rejected`, `kavita_refused`, `email_off` |
| `kavita_mcp_request_failures_total` | `elicitation_unsupported` |
| `kavita_mcp_auth_failures_total` | `mcp_token` |
| `kavita_mcp_search_skipped_total` | `library_not_granted` (Kavita and the configuration disagree: worth an alert) |
| `kavita_mcp_send_skipped_total` | `unknown_handle`, `no_kindle`, `format`, `too_large`, `declined` (user outcomes) |

## Development

```bash
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
bash scripts/set-version.test.sh
```

Kavita is a wiremock server inside every test, so the suite needs nothing external and nothing in
it can send a book. Releases are semantic-release on `main`; a release publishes the image.

## Contributing

See [contributing.md](contributing.md) and the [code of conduct](code-of-conduct.md). Report a
security problem privately, as [security.md](security.md) explains; changes are in
[changelog.md](changelog.md).

## License

[MIT](LICENSE)
