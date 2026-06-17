# Whisker Drawapp

A [Whisker](https://github.com/whiskerrs/whisker) app.

## Develop

```sh
# On an iOS Simulator (macOS only).
whisker run ios

# On an Android device or emulator.
whisker run android
```

Run `whisker doctor` first to verify your toolchain is set up for each
target.

## Edit

The UI lives in [`src/lib.rs`](src/lib.rs). Save any change and
`whisker run` hot-patches the running app in under a second — no
restart, no state loss.

App-level metadata (bundle id, app name, Android / iOS deployment
settings) lives in [`whisker.rs`](whisker.rs). Edits there require
a full `whisker run` restart since they shape the generated native
project.

## Build for release

Whisker doesn't wrap release builds — drive xcodebuild / gradle the
same way CI does:

```sh
# Android release APK
( cd gen/android && ./gradlew :app:assembleRelease )

# iOS Simulator .app (Release configuration)
xcodebuild -project gen/ios/<Scheme>.xcodeproj \
  -scheme <Scheme> -configuration Release \
  -destination 'generic/platform=iOS Simulator' build
```

The `gen/` tree is refreshed automatically on every `whisker run`;
delete it whenever you want a clean re-generate.
