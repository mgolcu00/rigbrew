---
name: ios-simulator
description: Build an iPhone or iPad app, run it in the iOS simulator, look at its screen and read its log. Use for Xcode, SwiftUI, Flutter or Expo apps on a Mac.
metadata:
  where: code
---

# Run and see an iOS app

The goal is the app on the simulator's screen, seen by you, with no errors in its log.

## Steps

1. **Is a simulator running?** Call `simulator` with `"action": "list"`. If none is booted, start
   one with `run_command`: `xcrun simctl boot "iPhone 16"` (pick a name from
   `xcrun simctl list devices available`). Starting one is the person's choice: they are asked.
2. **Know the project.** `list_files` at depth 1:
   - `*.xcodeproj` or `*.xcworkspace`: an Xcode project. Schemes: `xcodebuild -list`.
   - `pubspec.yaml`: Flutter. `app.json` with `expo`: Expo.
3. **Build and run** with `run_command`. Builds are slow: one command, then read its end.
   - Xcode: `xcodebuild -scheme <scheme> -destination 'platform=iOS Simulator,name=<device>' -derivedDataPath build -clonedSourcePackagesDirPath build/SourcePackages -packageCachePath build/PackageCache -IDEPackageSupportDisableManifestSandbox=YES build`
     (commands may write only inside the folder, so everything Xcode makes goes under `build/`)
     then `xcrun simctl install booted build/Build/Products/Debug-iphonesimulator/<App>.app`
     and `xcrun simctl launch booted <bundle id>`.
   - Flutter: `flutter build ios --simulator --debug`, then install and launch as above
     (the app is under `build/ios/iphonesimulator/`).
   - Expo: `npx expo run:ios` needs a terminal that stays open. Ask the person to run it.
4. **A build that fails**: read the first line with `error:`. It names a file and a line. Fix that
   one, build again. Do not read the whole log.
5. **Look.** `simulator` with `"action": "screenshot"`. Is the screen the one that was asked for?
   Text cut off, a blank screen and an overlap are faults.
6. **Read the log.** `simulator` with `"action": "logs"` and `"app"`: the app's name. A crash shows
   as `Fatal error` or `Terminating app`, with the reason on the same line.
7. **Fix one thing**, build and run again, look again.

## Tests

- Unit tests of a Swift package: `check_project`.
- An Xcode project: the build command above with `test` in place of the last `build`.
- Flutter: `check_project` runs `flutter test`.

## Rules

- Never erase or delete a simulator, and do not shut down one you did not boot.
- A command that runs for ever (`flutter run`, `expo start`) does not fit `run_command`: it stops
  after two minutes. Use the build, install and launch commands above.
- No screenshot possible: say so, and go by the log.
- A build that fails with "Operation not permitted" tried to write outside the folder. Say which
  path it was; the person can run that build in a terminal.

## The answer

What is on the screen, what the log says, what you fixed, and what you could not check.
