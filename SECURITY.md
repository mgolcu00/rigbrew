# Security

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private reporting:
**Security → Report a vulnerability** on this repository. You will get an answer within a few days.

Include what you found, how to reproduce it, and what an attacker could do with it.

## What is in scope

- `rigbrewd`, the local service: authentication of local requests, the data folder, engine supervision.
- The signed manifest and the downloader: anything that makes the app run or keep a file that does not match the
  manifest.
- The app: anything that lets a web page or another program act through it.
- Usage data: anything that sends more than [the documented layers](docs/api.md#what-leaves-the-machine).

## How the app protects you

See [Trust in the architecture notes](docs/architecture.md#trust): loopback-only services with a random token,
a private data folder, a signed manifest with a compiled-in key, sha256 checks on every file, safe unpacking of
engine archives.
