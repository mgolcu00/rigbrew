---
name: android-emulator
description: Build an Android app, run it on the emulator or a connected phone, look at its screen and read its log. Use for Gradle, Kotlin, Compose, Flutter or React Native apps.
metadata:
  where: code
---

# Run and see an Android app

The goal is the app on the device's screen, seen by you, with no crash in its log.

## Steps

1. **Is a device there?** Call `simulator` with `"action": "list"`. An emulator shows as
   `emulator-5554`. None: ask the person to start one from Android Studio, or to plug in a phone
   with USB debugging on. Do not try to create an emulator yourself.
2. **Know the project.** `list_files` at depth 1:
   - `gradlew` and `app/`: a Gradle project.
   - `pubspec.yaml`: Flutter. `android/` next to `package.json`: React Native.
3. **Build and install** with `run_command`. Builds are slow: one command, then read its end.
   - Gradle: `./gradlew installDebug`
   - Flutter: `flutter build apk --debug` then `adb install -r build/app/outputs/flutter-apk/app-debug.apk`
   - React Native: `cd android && ./gradlew installDebug` is not allowed (no `cd`). Use
     `./android/gradlew -p android installDebug`.
4. **Start it**: `adb shell monkey -p <application id> 1`. The application id is `applicationId` in
   `app/build.gradle` or `app/build.gradle.kts`.
5. **A build that fails**: read the first line that starts with `e:` or holds `error:`. It names a
   file and a line. Fix that one, build again.
6. **Look.** `simulator` with `"action": "screenshot"`. Is the screen the one that was asked for?
7. **Read the log.** `simulator` with `"action": "logs"` and `"app"`: the application id. A crash
   shows as `FATAL EXCEPTION`, and the lines under it name the exception and the file.
8. **Fix one thing**, install and start again, look again.

## Tests

- Unit tests: `./gradlew testDebugUnitTest`. Flutter: `check_project` runs `flutter test`.
- Tests on the device: `./gradlew connectedDebugAndroidTest`. They need the device from step 1.

## Rules

- Do not wipe the device or uninstall other apps.
- A command that runs for ever (`flutter run`, `npx react-native start`) does not fit
  `run_command`: it stops after two minutes. Ask the person to run it in a terminal.
- No screenshot possible: say so, and go by the log.

## The answer

What is on the screen, what the log says, what you fixed, and what you could not check.
