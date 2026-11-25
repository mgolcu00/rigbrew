//! The phone app as it runs: the booted iOS simulators (macOS, with Xcode) and the Android
//! devices `adb` sees. Three things can be asked: which devices there are, a picture of one's
//! screen, and the last lines of its log.
//!
//! These are fixed commands the service runs itself, outside the folder's sandbox (the
//! simulator cannot be reached from inside it): `xcrun simctl list / io screenshot / spawn log
//! show` and `adb devices / exec-out screencap / logcat -d`. Nothing the model writes becomes a
//! command line: a device must be one of those listed, and an app's name may only hold
//! letters, digits, dots, dashes and underscores. They only read, so every mode has them,
//! Read only included, and none of them asks first. Building and starting the app is the
//! model's own work through `run_command`, with the person's yes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::files;
use crate::connectors::find_program;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Lines of a log the model is shown.
const LOG_LINES: usize = 60;
/// The longer side of a screenshot, in pixels, after it was made smaller (macOS).
const SHOT_SIDE: u32 = 1280;
const MAX_SHOT_BYTES: usize = 12 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
}

/// The last picture of a phone's screen a task took, as it is kept with the task and shown
/// in the preview.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Screen {
    /// The picture's name under the task's shots.
    pub shot: String,
    /// The device as it is shown: "iPhone 16 Pro (iOS 18.2, simulator)".
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub at_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub platform: Platform,
    /// The simulator's UDID, or the serial `adb` knows the device by.
    pub id: String,
    pub name: String,
    /// "iOS 18.2", or empty when it is not known.
    pub system: String,
}

impl Device {
    pub fn shown(&self) -> String {
        match (self.platform, self.system.is_empty()) {
            (Platform::Ios, false) => format!("{} ({}, simulator)", self.name, self.system),
            (Platform::Ios, true) => format!("{} (simulator)", self.name),
            (Platform::Android, _) => format!("{} (Android)", self.name),
        }
    }
}

/// The booted simulators in the answer of `xcrun simctl list devices booted -j`.
pub fn parse_simctl(text: &str) -> Vec<Device> {
    let Ok(answer) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let mut out = Vec::new();
    for (runtime, devices) in answer["devices"].as_object().into_iter().flatten() {
        // "com.apple.CoreSimulator.SimRuntime.iOS-18-2" is "iOS 18.2".
        let system = runtime.rsplit('.').next().unwrap_or("").replacen('-', " ", 1).replace('-', ".");
        for device in devices.as_array().into_iter().flatten() {
            let (Some(id), Some(name)) = (device["udid"].as_str(), device["name"].as_str()) else { continue };
            if device["state"] == "Booted" && valid_id(id) {
                out.push(Device { platform: Platform::Ios, id: id.to_string(), name: name.to_string(), system: system.clone() });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The devices that are ready in the answer of `adb devices -l`.
pub fn parse_adb(text: &str) -> Vec<Device> {
    let mut out = Vec::new();
    for line in text.lines().skip_while(|l| !l.starts_with("List of devices")).skip(1) {
        let mut words = line.split_whitespace();
        let (Some(id), Some("device")) = (words.next(), words.next()) else { continue };
        if !valid_id(id) {
            continue;
        }
        let model = words.find_map(|w| w.strip_prefix("model:")).map(|m| m.replace('_', " "));
        out.push(Device {
            platform: Platform::Android,
            id: id.to_string(),
            name: model.unwrap_or_else(|| id.to_string()),
            system: String::new(),
        });
    }
    out
}

/// A UDID or an adb serial: nothing that could be read as an option or a second argument.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':'))
}

/// An app's process name or its id ("Shop", "dev.rigbrew.shop").
pub fn valid_app(app: &str) -> bool {
    !app.is_empty()
        && app.len() <= 100
        && !app.starts_with('-')
        && app.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// `xcrun`, when the developer tools are set up: asking without them would open a dialog.
#[cfg(target_os = "macos")]
fn xcrun() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            let selected = std::process::Command::new("/usr/bin/xcode-select").arg("-p").output().ok()?;
            let dir = PathBuf::from(String::from_utf8_lossy(&selected.stdout).trim());
            // The simulator comes with Xcode itself, not with the command line tools alone.
            (selected.status.success() && dir.join("usr/bin/simctl").is_file()).then(|| PathBuf::from("/usr/bin/xcrun"))
        })
        .clone()
}

#[cfg(not(target_os = "macos"))]
fn xcrun() -> Option<PathBuf> {
    None
}

fn adb() -> Option<PathBuf> {
    find_program("adb")
}

/// Whether this computer can show a phone app at all.
pub fn available() -> bool {
    xcrun().is_some() || adb().is_some()
}

/// Whether the folder holds a phone app: an Xcode project, a Flutter or React Native app, an
/// Android project.
pub fn mobile_project(root: &Path) -> bool {
    let named = |test: &dyn Fn(&str) -> bool| {
        std::fs::read_dir(root).into_iter().flatten().flatten().take(500).any(|e| test(&e.file_name().to_string_lossy()))
    };
    let text = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    named(&|n| n.ends_with(".xcodeproj") || n.ends_with(".xcworkspace"))
        || root.join("ios").is_dir()
        || root.join("android").is_dir()
        || root.join("app/src/main/AndroidManifest.xml").is_file()
        || (root.join("pubspec.yaml").is_file() && text("pubspec.yaml").contains("flutter"))
        || ["\"react-native\"", "\"expo\""].iter().any(|k| text("package.json").contains(k))
        || (root.join("Package.swift").is_file() && text("Package.swift").contains(".iOS("))
}

/// Runs one fixed program and gives what it wrote.
async fn fixed(program: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let name = program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
    if let Ok(path) = std::env::join_paths(crate::connectors::search_dirs()) {
        cmd.env("PATH", path);
    }
    let out = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{name} did not answer within {} seconds.", TIMEOUT.as_secs()))?
        .map_err(|e| format!("{name} could not be started: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it gave no reason");
        return Err(format!("{name} did not work: {}", said.chars().take(200).collect::<String>()));
    }
    Ok(out.stdout)
}

/// Every booted simulator and connected Android device.
pub async fn list() -> Vec<Device> {
    let mut out = Vec::new();
    if let Some(xcrun) = xcrun() {
        if let Ok(answer) = fixed(&xcrun, &["simctl", "list", "devices", "booted", "-j"]).await {
            out.extend(parse_simctl(&String::from_utf8_lossy(&answer)));
        }
    }
    if let Some(adb) = adb() {
        if let Ok(answer) = fixed(&adb, &["devices", "-l"]).await {
            out.extend(parse_adb(&String::from_utf8_lossy(&answer)));
        }
    }
    out
}

pub const NONE: &str =
    "No simulator is booted and no Android device is connected. Start one first: use_skill ios-simulator or android-emulator says how.";

/// The device a call means: the one named (by name or id, in any case), or the only one, or
/// the first. An error when there is none, or none by that name.
pub fn pick<'a>(devices: &'a [Device], wanted: Option<&str>) -> Result<&'a Device, String> {
    let wanted = wanted.map(str::trim).filter(|w| !w.is_empty());
    match wanted {
        _ if devices.is_empty() => Err(NONE.into()),
        None => Ok(&devices[0]),
        Some(w) => devices
            .iter()
            .find(|d| d.id.eq_ignore_ascii_case(w) || d.name.eq_ignore_ascii_case(w))
            .or_else(|| devices.iter().find(|d| d.name.to_lowercase().contains(&w.to_lowercase())))
            .ok_or_else(|| format!("There is no device named {w}. The devices are: {}.", names(devices))),
    }
}

pub fn names(devices: &[Device]) -> String {
    devices.iter().map(Device::shown).collect::<Vec<_>>().join(", ")
}

pub fn listed(devices: &[Device]) -> String {
    if devices.is_empty() {
        return NONE.into();
    }
    let mut out = String::from("Devices that are running:");
    for d in devices {
        out.push_str(&format!("\n- {}", d.shown()));
    }
    out
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

/// A picture of the device's screen, as a PNG no larger than a model needs.
pub async fn screenshot(device: &Device, scratch: &Path) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("The picture could not be kept: {e}"))?;
    let file = scratch.join(format!("shot-{}.png", uuid::Uuid::new_v4().simple()));
    let path = file.to_string_lossy().into_owned();
    let taken = match device.platform {
        Platform::Ios => {
            let xcrun = xcrun().ok_or("Xcode is not set up on this computer.")?;
            fixed(&xcrun, &["simctl", "io", &device.id, "screenshot", "--type=png", &path]).await.map(|_| ())
        }
        Platform::Android => {
            let adb = adb().ok_or("adb is not installed on this computer.")?;
            match fixed(&adb, &["-s", &device.id, "exec-out", "screencap", "-p"]).await {
                Ok(png) => std::fs::write(&file, png).map_err(|e| format!("The picture could not be kept: {e}")),
                Err(e) => Err(e),
            }
        }
    };
    if let Err(why) = taken {
        std::fs::remove_file(&file).ok();
        return Err(why);
    }
    // A phone's screen has more pixels than a model reads: made smaller with the system's own tool.
    if cfg!(target_os = "macos") && Path::new("/usr/bin/sips").is_file() {
        fixed(Path::new("/usr/bin/sips"), &["-Z", &SHOT_SIDE.to_string(), &path]).await.ok();
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("The picture could not be read: {e}"));
    std::fs::remove_file(&file).ok();
    let bytes = bytes?;
    if !is_png(&bytes) || bytes.len() > MAX_SHOT_BYTES {
        return Err("The device did not give a picture.".into());
    }
    Ok(bytes)
}

/// The end of a log, short enough for the model.
pub fn tail(text: &str, only: Option<&str>) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !l.starts_with("Timestamp ") && !l.starts_with("---------"))
        .filter(|l| only.is_none_or(|app| l.to_lowercase().contains(&app.to_lowercase())))
        .collect();
    if lines.is_empty() {
        return "(no lines)".into();
    }
    let kept: Vec<String> = lines.iter().skip(lines.len().saturating_sub(LOG_LINES)).map(|l| l.chars().take(300).collect()).collect();
    files::cap_ends(&kept.join("\n"), files::RESULT_CHARS)
}

/// The last log lines of a device: those of one app when it is named, the errors otherwise.
pub async fn logs(device: &Device, app: Option<&str>) -> Result<String, String> {
    let app = app.map(str::trim).filter(|a| !a.is_empty());
    if let Some(app) = app {
        if !valid_app(app) {
            return Err("\"app\" is the app's name or id: letters, digits, dots, dashes and underscores only.".into());
        }
    }
    match device.platform {
        Platform::Ios => {
            let xcrun = xcrun().ok_or("Xcode is not set up on this computer.")?;
            // The name was checked above: it holds nothing that ends the quoted text.
            let predicate = match app {
                Some(app) => format!("process == \"{app}\" OR subsystem == \"{app}\" OR senderImagePath CONTAINS[c] \"{app}\""),
                None => "messageType == error OR messageType == fault".to_string(),
            };
            let args = ["simctl", "spawn", &device.id, "log", "show", "--last", "1m", "--style", "compact", "--predicate", &predicate];
            let out = fixed(&xcrun, &args).await?;
            let what = app.map_or("errors".to_string(), |a| format!("lines of {a}"));
            Ok(format!("Log of {} (the last minute, {what}):\n{}", device.shown(), tail(&String::from_utf8_lossy(&out), None)))
        }
        Platform::Android => {
            let adb = adb().ok_or("adb is not installed on this computer.")?;
            let level = if app.is_some() { "*:V" } else { "*:W" };
            let out = fixed(&adb, &["-s", &device.id, "logcat", "-d", "-t", "200", level]).await?;
            let what = app.map_or("warnings and errors".to_string(), |a| format!("lines that name {a}"));
            Ok(format!("Log of {} (the last 200 lines, {what}):\n{}", device.shown(), tail(&String::from_utf8_lossy(&out), app)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `xcrun simctl list devices booted -j` prints (Xcode 16), shortened.
    const SIMCTL: &str = r#"{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.iOS-17-5" : [

    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-18-2" : [
      {
        "lastBootedAt" : "2026-11-25T09:12:44Z",
        "dataPath" : "\/Users\/someone\/Library\/Developer\/CoreSimulator\/Devices\/6E3B1B6C-6C0C-4F6B-9A55-0C7F3D0B5C11\/data",
        "dataPathSize" : 1893642240,
        "logPath" : "\/Users\/someone\/Library\/Logs\/CoreSimulator\/6E3B1B6C-6C0C-4F6B-9A55-0C7F3D0B5C11",
        "udid" : "6E3B1B6C-6C0C-4F6B-9A55-0C7F3D0B5C11",
        "isAvailable" : true,
        "logPathSize" : 421888,
        "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-16-Pro",
        "state" : "Booted",
        "name" : "iPhone 16 Pro"
      },
      {
        "udid" : "11111111-2222-3333-4444-555555555555",
        "isAvailable" : true,
        "state" : "Shutdown",
        "name" : "iPhone SE (3rd generation)"
      }
    ],
    "com.apple.CoreSimulator.SimRuntime.iOS-26-0" : [
      {
        "udid" : "A0A0A0A0-B1B1-C2C2-D3D3-E4E4E4E4E4E4",
        "isAvailable" : true,
        "state" : "Booted",
        "name" : "iPad Air 11-inch (M3)"
      },
      {
        "udid" : "--erase all",
        "state" : "Booted",
        "name" : "not a device"
      }
    ]
  }
}"#;

    const ADB: &str = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\nemulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1\nR58M123ABC             unauthorized usb:1-1 transport_id:2\n192.168.1.20:5555      device product:panther model:Pixel_7 device:panther transport_id:3\nold-phone              offline\n\n";

    #[test]
    fn the_running_devices_are_read_from_what_the_tools_print() {
        let sims = parse_simctl(SIMCTL);
        assert_eq!(sims.len(), 2, "only booted ones, and only real ids: {sims:?}");
        assert_eq!(sims[0].shown(), "iPad Air 11-inch (M3) (iOS 26.0, simulator)");
        assert_eq!(sims[1].shown(), "iPhone 16 Pro (iOS 18.2, simulator)");
        assert_eq!(sims[1].id, "6E3B1B6C-6C0C-4F6B-9A55-0C7F3D0B5C11");
        assert!(parse_simctl("xcrun: error: unable to find utility \"simctl\"").is_empty());
        assert!(parse_simctl(r#"{"devices": {}}"#).is_empty());
        let phones = parse_adb(ADB);
        assert_eq!(phones.len(), 2, "{phones:?}");
        assert_eq!((phones[0].id.as_str(), phones[0].shown().as_str()), ("emulator-5554", "sdk gphone64 arm64 (Android)"));
        assert_eq!((phones[1].id.as_str(), phones[1].name.as_str()), ("192.168.1.20:5555", "Pixel 7"));
        assert!(parse_adb("List of devices attached\n\n").is_empty());
        assert_eq!(parse_adb("List of devices attached\nemulator-5554\tdevice\n")[0].name, "emulator-5554");
    }

    #[test]
    fn a_device_is_chosen_by_name_and_nothing_else_becomes_an_argument() {
        let mut all = parse_simctl(SIMCTL);
        all.extend(parse_adb(ADB));
        assert_eq!(pick(&all, None).unwrap().name, "iPad Air 11-inch (M3)");
        assert_eq!(pick(&all, Some("iphone 16 pro")).unwrap().id, "6E3B1B6C-6C0C-4F6B-9A55-0C7F3D0B5C11");
        assert_eq!(pick(&all, Some("6e3b1b6c-6c0c-4f6b-9a55-0c7f3d0b5c11")).unwrap().name, "iPhone 16 Pro");
        assert_eq!(pick(&all, Some("pixel")).unwrap().id, "192.168.1.20:5555");
        assert_eq!(pick(&all, Some("  ")).unwrap().name, "iPad Air 11-inch (M3)");
        let unknown = pick(&all, Some("booted; rm -rf ~")).unwrap_err();
        assert!(unknown.starts_with("There is no device named booted; rm -rf ~. The devices are: iPad Air"), "{unknown}");
        assert_eq!(pick(&[], None).unwrap_err(), NONE);
        assert_eq!(listed(&[]), NONE);
        assert!(listed(&all).starts_with("Devices that are running:\n- iPad Air 11-inch (M3) (iOS 26.0, simulator)\n- iPhone 16 Pro"));
        for good in ["Shop", "dev.rigbrew.shop", "my-app_2"] {
            assert!(valid_app(good), "{good}");
        }
        for bad in ["", "-f", "Shop\" OR 1 == 1 OR \"", "a b", "x;y", "$(id)", "a/b"] {
            assert!(!valid_app(bad), "{bad}");
        }
        for bad in ["", "-s", "a b", "x;reboot", "$(id)"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_log_is_cut_to_its_end_and_an_app_name_is_checked_first() {
        let log: String = (0..300).map(|i| format!("2026-11-25 10:00:{:02}.000 E  Shop[123:456] line {i}\n", i % 60)).collect();
        let shown = tail(&format!("Timestamp               Ty Process[PID:TID]\n{log}\n"), None);
        assert!(shown.lines().filter(|l| l.contains("Shop[")).count() <= LOG_LINES);
        assert!(shown.ends_with("line 299") && !shown.contains("Timestamp"), "{shown}");
        assert_eq!(tail("\n\n", None), "(no lines)");
        let logcat = "11-25 10:00:00.000  1234  1234 E AndroidRuntime: FATAL EXCEPTION: main\n11-25 10:00:00.001  1234  1234 E AndroidRuntime: Process: dev.rigbrew.shop, PID: 1234\n11-25 10:00:00.002   999   999 W Other: noise\n";
        assert_eq!(
            tail(logcat, Some("dev.rigbrew.shop")),
            "11-25 10:00:00.001  1234  1234 E AndroidRuntime: Process: dev.rigbrew.shop, PID: 1234"
        );
        let device = Device { platform: Platform::Android, id: "emulator-5554".into(), name: "Pixel".into(), system: String::new() };
        let refused = logs(&device, Some("x\" OR \"1")).await.unwrap_err();
        assert!(refused.starts_with("\"app\" is the app's name or id"), "{refused}");
        assert!(is_png(&[0x89, b'P', b'N', b'G', 13, 10]) && !is_png(b"<html>"));
    }

    #[test]
    fn a_phone_project_is_told_from_the_rest() {
        let project = |files_in_it: &[(&str, &str)]| {
            let root = files::scratch("devices");
            for (path, text) in files_in_it {
                std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
                std::fs::write(root.join(path), text).unwrap();
            }
            mobile_project(&root)
        };
        assert!(project(&[("Shop.xcodeproj/project.pbxproj", "")]));
        assert!(project(&[("pubspec.yaml", "dependencies:\n  flutter:\n    sdk: flutter\n")]));
        assert!(project(&[("package.json", r#"{"dependencies": {"expo": "~52.0.0", "react-native": "0.76.0"}}"#)]));
        assert!(project(&[("app/src/main/AndroidManifest.xml", "<manifest/>")]));
        assert!(project(&[("android/build.gradle", "")]));
        assert!(!project(&[("package.json", r#"{"dependencies": {"svelte": "^5"}}"#), ("src/app.html", "")]));
        assert!(!project(&[("Cargo.toml", "[package]\n")]));
    }
}
