# Roadmap

Where Rigbrew is going, in order. Dates are not promised; the order is.

## Now

- **macOS beta.** A signed and notarized app with automatic updates, packages served from `models.rigbrew.dev`.
- **Windows.** The same app on Windows 11: NVIDIA (CUDA), AMD and Intel (Vulkan) and CPU engines, GPU detection
  through DXGI.

## Done recently

- **Read aloud.** Whistle reads any answer aloud, in English, on the processor, next to the loaded model.
- **One-command release build** with the local service inside the app (`pnpm release`).

## Next

- **Listening in the chat.** Talk instead of typing (Sift), and more voices and languages for Whistle.
- **Video in the chat.** Pour on machines with enough memory, with honest estimates of how long a clip takes.
- **Two models in memory** on machines that can hold them, so a picture asked for in chat does not unload the chat
  model.
- **More sizes per family**, from very small machines (8 GB, no graphics card) to workstations.

## Later

- **Linux:** the command line first, then the app.
- **Measured on your machine:** speed reported back per hardware class, so recommendations get better with use.
- **Rigbrew's own model builds**, trained and distilled for small machines, published with the same measurements.

## Not planned

- Running models on Rigbrew's servers. Everything runs on your machine.
- Ads.

Ideas and requests: open an issue. If something on this list matters to you, say so on its issue; that is how the
order changes.
