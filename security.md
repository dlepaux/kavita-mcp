# Security policy

## Supported versions

| Version | Supported          |
| ------- | ------------------ |
| 1.x     | :white_check_mark: |
| < 1.0   | :x:                |

Fixes go into the latest release; upgrade to it rather than asking for a backport.

## Reporting a vulnerability

Please report it privately, through GitHub's private vulnerability reporting: **Security → Report a
vulnerability** on this repository
(<https://github.com/dlepaux/kavita-mcp/security/advisories/new>). Do not open a public issue.

### What to include

- the kavita-mcp version (`kavita-mcp --version`) and the Kavita version;
- what you did, what you expected and what happened;
- the impact as you see it, and a fix if you have one.

### What happens next

- **Acknowledgement:** within 7 days.
- **Assessment and plan:** in the advisory, as soon as the issue is understood.
- **Fix:** timing depends on severity. The advisory is published when the fixed release is out,
  crediting you if you wish.

If the issue is in Kavita itself rather than in kavita-mcp, please report it to the Kavita project
privately as well.

## Security model

What kavita-mcp guarantees, and what it never logs, is in the readme's
[security model](readme.md#security-model).
