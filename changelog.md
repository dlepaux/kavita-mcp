## [1.1.4](https://github.com/dlepaux/kavita-mcp/compare/v1.1.3...v1.1.4) (2026-10-30)

## [1.1.3](https://github.com/dlepaux/kavita-mcp/compare/v1.1.2...v1.1.3) (2026-10-29)

## [1.1.2](https://github.com/dlepaux/kavita-mcp/compare/v1.1.1...v1.1.2) (2026-10-27)

## [1.1.1](https://github.com/dlepaux/kavita-mcp/compare/v1.1.0...v1.1.1) (2026-10-22)

## [1.1.0](https://github.com/dlepaux/kavita-mcp/compare/v1.0.4...v1.1.0) (2026-10-21)

### Features

* word what the person reads in the language they wrote in ([#6](https://github.com/dlepaux/kavita-mcp/issues/6)) ([0e91f78](https://github.com/dlepaux/kavita-mcp/commit/0e91f78b7d0572c1a0c1f2446c1ebf3590959a9d))

## [1.0.4](https://github.com/dlepaux/kavita-mcp/compare/v1.0.3...v1.0.4) (2026-10-20)

## [1.0.3](https://github.com/dlepaux/kavita-mcp/compare/v1.0.2...v1.0.3) (2026-10-20)

## [1.0.2](https://github.com/dlepaux/kavita-mcp/compare/v1.0.1...v1.0.2) (2026-10-20)

## [1.0.1](https://github.com/dlepaux/kavita-mcp/compare/v1.0.0...v1.0.1) (2026-10-19)

### Bug Fixes

* tell the model the action tool asks the person itself ([2db6490](https://github.com/dlepaux/kavita-mcp/commit/2db6490c5002bb5a392c2f63ebb311648da0f9b6))

## 1.0.0 (2026-10-18)

### Features

* **config:** identities, per-person Kavita keys, fail fast on a missing or orphan key
* **handles:** per-person search handles, 60 min, 50 each, letters only
* **http:** /mcp with bearer identities, the two tools, /metrics and /healthz
* **kavita:** client that sends the key only as x-api-key and maps statuses to kinds
* **messages:** every kavita-mcp sentence in French and English
* **metrics:** the five kavita-mcp counters, every kind at 0 from start
* **tools:** search_library with per-person handles and the library access check
* **tools:** send_to_kindle confirms by elicitation and refuses before sending

### Bug Fixes

* word Kavita faults by step, and promise no author search
