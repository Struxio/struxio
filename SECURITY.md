# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| `main` (latest) | ✅ Yes |
| Older releases | ❌ No |

We only provide security fixes for the latest version on the `main` branch.

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in Struxio, please report it responsibly using one of the following methods:

### Option 1: GitHub Private Security Advisory (preferred)

Use GitHub's built-in private disclosure feature:
👉 [Report a vulnerability](../../security/advisories/new)

This creates a private, encrypted channel between you and the maintainers.

### Option 2: Email

Send details to: **security@struxio.app**

Please include:
- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested mitigations (optional)

## Response Timeline

| Stage | Timeline |
|-------|----------|
| Acknowledgement | Within **48 hours** |
| Initial assessment | Within **5 business days** |
| Fix or mitigation | Within **30 days** (critical), **90 days** (non-critical) |

## Disclosure Policy

We follow [Coordinated Vulnerability Disclosure (CVD)](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure). We will:
1. Confirm the vulnerability and assess its impact
2. Develop and test a fix
3. Release the fix
4. Credit you in the release notes (unless you prefer to remain anonymous)

## Out of Scope

The following are generally **not** considered security vulnerabilities:
- Vulnerabilities in third-party dependencies (please report those upstream)
- Rate limiting on non-authenticated endpoints
- Missing security headers on non-sensitive pages
- Self-XSS

## Thank You

We appreciate responsible security researchers who help keep Struxio secure. Security researchers who report valid vulnerabilities will be credited in our release notes.
