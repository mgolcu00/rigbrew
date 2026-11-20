# Model packages

Rigbrew does not point you at a model hub. It ships its own builds of open-weight models, made for classes of
hardware, measured against the original, and listed in a signed manifest.

## Families

Each family does one thing. Installed families work together in the same chat: the chat model can ask the picture
model for an image, and the app shows which model did what.

| Family | Does | First package | In the app |
|---|---|---|---|
| **Steep** | Chat, writing, explaining; can call the other families | Steep 4B | Yes |
| **Crema** | Pictures from a description | Crema 6B | Yes |
| **Pour** | Short video clips from a description | Pour 5B | Download now, running it comes later |
| **Whistle** | Reading answers aloud, in English | Whistle 82M | Yes |
| **Sift** | Listening: speech to text | — | Planned |

Package ids are `<family>-<size>` (`steep-4b`); files are `<id>-v<version>.<part>.<precision>.<ext>`. People see
versions by what they mean ("Smallest", "Balanced", "Best quality", "Recommended for this Mac"); the precision tags
(`q4`, `q5`, `int8`) are shown only in the Pro view.

## How a build is measured

A version is published with numbers, not adjectives. Each one is compared with the full-precision original on the
same inputs:

- **Chat:** KL divergence of the next-token distribution and how often the most likely next token is the same,
  over a fixed text set; plus speed on reference machines.
- **Pictures:** LPIPS, SSIM and PSNR against the full-precision model with the same prompts and seeds, plus a look
  at fine detail and text in the images.
- **Voice:** word error rate after transcribing the generated speech, and real-time factor.

Example: the chat family's versions against the original.

| Version | Size | KL divergence | Same next token |
|---|---|---|---|
| Best quality | 4.6 GB | 0.0005 | 99.1% |
| Better | 3.7 GB | 0.0026 | 97.3% |
| Balanced | 3.3 GB | 0.0066 | 96.4% |
| Small | 3.0 GB | 0.0202 | 93.2% |
| Smallest | 2.5 GB | 0.0819 | 86.9% |

The selector uses these numbers: it picks the lightest version that clears the family's quality bar and fits the
machine.

## Licences

Only models under Apache-2.0, MIT or CC-BY-4.0 are packaged. Every package ships the licence of the model it is
built from and a `NOTICE` naming its origin and what was changed; the app shows the licence. Rigbrew is not
affiliated with the authors of those models. If you think a package is missing an attribution, open an issue.

## Asking for a model

Open an issue with the "Model request" template: what you want to do, the model, its licence, and your hardware.
