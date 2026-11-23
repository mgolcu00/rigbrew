# Privacy Policy

Draft of 2026-11-23 (revision 3: the waiting list on rigbrew.dev). Not reviewed by a lawyer.

## Who is responsible

[OPERATOR NAME, LEGAL FORM, ADDRESS], contact: [OPERATOR E-MAIL].

## What Rigbrew is

A desktop app that downloads open AI models and runs them on your computer. The models run locally. The app
connects to the internet to download model packages, to check for updates, to sign you in if you choose to, and to
send the usage data described here.

## Data we collect through the app

Two layers.

**Basic usage data, always.** Collected from the first start of the app, whether or not you accept the consent
text, and also after you withdraw it. It never contains what you type or what models answer and is not linked to
an account.

| Data | Examples | Why |
|---|---|---|
| Usage events | App installed and opened, screen viewed, main buttons used, mode changed, package download started, finished, failed, removed; engine started; update shown; sponsor link opened; consent shown, accepted, withdrawn | Know how many people use Rigbrew and what they do |
| Sizes, not words | Number of messages, number of characters, names of tools a model used | Understand usage without reading it |
| Performance | Tokens per second, seconds per picture, load times, errors (shortened) | Measure real speed per hardware class |
| Hardware profile | Operating system and version, processor model, memory, GPU model and memory, free disk space, hardware class | Fit models to machines |
| Identifiers | A random installation ID and a session ID per run | Count installs and sessions |
| Network | Country, derived by our network provider from your IP address. The IP address itself is not stored. | Rough geography |

Developers can switch this layer off by starting the app with the environment variable `DO_NOT_TRACK=1`.

**Content, only with your consent.** While your consent stands:

| Data | Examples | Why |
|---|---|---|
| Prompts and responses | The full text you send to chat models and the full text they answer; prompts for pictures; tool calls made by the model | Improve model builds and selection; train and evaluate models; find failures |
| Account link | A Firebase account ID (anonymous until you sign in); if you sign in, your e-mail address, display name and sign-in provider | Tie your data to your account so you can export or delete it; sign-in and account recovery |

Generated pictures, audio and video are not uploaded. Files on your computer are not read, except the model
packages Rigbrew itself downloaded.

## Data we collect on rigbrew.dev

Page views without cookies: page, referrer, screen size, language, time zone, country, device class, and a visitor
hash that changes every day and cannot be turned back into an IP address. Clicks on download and sponsor links.

## Waiting list

If you join the waiting list on rigbrew.dev we keep the e-mail address you give, and, only if you answer them, which
computer you use, how much memory it has and the chip or graphics card you type in. With it: the date, the site or
campaign you came from (a host name such as `t.co`, never a full address), your country as our network provider
derives it, and whether you signed up on a computer or a phone. Not your IP address.

We use it to send you an e-mail when a download is out (macOS first, then Windows) and to decide which hardware to
build and measure for first. Nothing else: no newsletter, no advertising, not shared with anyone. Every e-mail lets
you leave the list, and the form on rigbrew.dev takes an address off it at once.

## Where it goes

- **Our server**, hosted by Hetzner Online GmbH in Germany.
- **Cloudflare, Inc.** carries traffic to the server and the site, and hosts model package files.
- **Google Firebase Authentication** (Google LLC / Google Ireland Ltd.) handles sign-in and holds your account's
  e-mail address and identifiers.
- **GitHub** hosts the source code and, if you sponsor the project, handles the sponsorship; we receive what GitHub
  shares with sponsored projects.

We do not sell this data and do not use it for advertising.

## How long

Event data, including prompts and responses: up to 24 months, then deleted or made anonymous. Account data: until
you delete your account. The waiting list: until you leave it, and no longer than 12 months after the last
announcement it is kept for. Server logs: 30 days. Daily copies of the server's database, kept on the server for
recovery: 14 days.

## Your choices

- **See what is queued**: Settings → Privacy and data shows every event that has not been sent yet.
- **Withdraw consent**: same place. Content that has not been sent yet is removed from your computer and no new
  content is collected; basic usage data continues. The app cannot be used until you accept again.
- **Export** or **delete** what is stored on the server about your account: Settings → Account. Deleting removes the
  events linked to your account and installation and deletes the account.
- Depending on where you live you may also have rights of access, correction, objection, restriction, portability
  and complaint to a supervisory authority. Write to [OPERATOR E-MAIL].

## Sensitive information

Prompts can contain anything you type. Do not enter health data, passwords, payment details, government
identifiers or other people's personal information.

## Children

Rigbrew is not for anyone under 18.

## Changes

When this policy or the consent text changes in a way that matters, the app asks for consent again.
