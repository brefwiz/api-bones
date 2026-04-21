# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 3.x     | ✅        |
| < 3.0   | ❌        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Please report security issues by email to **security@brefwiz.com**.

Include:
- A description of the vulnerability
- Steps to reproduce or proof-of-concept
- Impact assessment if known

You can expect an acknowledgement within 48 hours and a resolution timeline within 7 days for critical issues.

## Scope

This crate includes cryptographic surface (HMAC-SHA256 cursor signing, Base64 credential handling, `zeroize`-protected secrets). These are the most sensitive areas; reports against them are treated as high priority.
