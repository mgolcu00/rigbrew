# Contributing

Thank you for helping. Rigbrew is early, so the most useful contributions right now are:

- **Bug reports with your hardware.** Open Rigbrew → *Your Mac* and include the class, memory and graphics in the
  issue. The bug report template asks for it.
- **Measurements.** How fast a model ran on your machine is valuable; the app's profile page shows it.
- **Small, focused pull requests.** For anything larger, open an issue first so we agree on the shape.

## Building

See [Build from source](README.md#build-from-source). Before you open a pull request:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cd app && pnpm check && pnpm build
```

## Guidelines

- One change per pull request, with a description of what you checked and how.
- New behaviour in `core/` or `daemon/` comes with a test.
- A claim about speed or quality needs the machine it was measured on and how.
- Text people see follows [the brand voice](docs/brand/README.md): short, concrete, measured. The interface is
  English; plain words in the Guided view, technical terms only in Standard and Pro.
- Usage data: never add anything that sends text people typed or model output outside the consented content layer
  ([`core/src/events.rs`](core/src/events.rs)). New event kinds use short labels and numbers only.

## Model requests

Use the "Model request" issue template. Only models under Apache-2.0, MIT or CC-BY-4.0 are packaged.

## Licence

By contributing you agree that your contribution is licensed under [Apache-2.0](LICENSE).
