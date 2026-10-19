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
