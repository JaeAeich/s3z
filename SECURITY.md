# Security Policy

## Supported versions

Only the latest release on `main` is actively supported with security fixes.

## Reporting a vulnerability

If you discover a security vulnerability in s3z, **please do not open a public
issue.** Instead, report it privately:

- Use [GitHub's private vulnerability reporting](https://github.com/JaeAeich/s3z/security/advisories/new)

Please include:

- A description of the vulnerability
- Steps to reproduce or a proof of concept
- The impact you believe this has

## Response timeline

- **Acknowledgement** within 48 hours of receipt.
- **Assessment and fix** targeting 7 days for critical issues, 30 days for
  lower-severity findings.
- A public advisory will be published once a fix is available.

## Scope

This policy covers the s3z library (`crates/core`), CLI (`crates/cli`), and
language bindings (`bindings/node`, `bindings/python`). Benchmark tooling and
example applications are out of scope.
