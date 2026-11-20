# Roadmap

Where Rigbrew is going, in order. Dates are not promised; the order is.

## Now

- **macOS beta.** A signed and notarized app with automatic updates, packages served from `models.rigbrew.dev`.
- **Windows.** The same app on Windows 11: NVIDIA (CUDA), AMD and Intel (Vulkan) and CPU engines, GPU detection
  through DXGI.

## Done recently

- **Read aloud.** Whistle reads any answer aloud, in English, on the processor, next to the loaded model.
- **Video in the chat.** Pour makes clips of up to three seconds in a few minutes, decoded by a tiny decoder in
  seconds instead of twenty minutes, which also keeps it within 16 GB Macs.
- **Two models in memory** where they fit: a picture asked for in a chat no longer unloads the chat model.
- **Pictures show their steps** and Stop stops the engine, not only the wait.
- **One-command release build** with the local service inside the app (`pnpm release`).

## Next

- **Listening in the chat.** Talk instead of typing (Sift), and more voices and languages for Whistle.
- **Longer and sharper clips**, and starting a clip from a picture.
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
