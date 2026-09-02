# CowPaper updater and data-preservation contract

This branch uses the official Tauri 2 updater plugin. The first release of
this flow is manual: Settings → 检查更新 → confirm → download and install.
There is no silent background download or automatic update check.

## Runtime design

- `tauri-plugin-updater` reads the signed static metadata from GitHub Releases:
  `https://github.com/hiahiayue-lab/CowPaper/releases/latest/download/latest.json`.
- The configured public key is embedded in `app/src-tauri/tauri.conf.json`.
  Tauri verifies the artifact signature before installation; invalid signatures
  are rejected by the plugin and are not bypassed by CowPaper.
- `@tauri-apps/plugin-process` relaunches the app after installation. Windows
  uses the official passive installer mode; macOS relaunches the newly replaced
  app bundle.
- A failed check, download, or signature verification is shown in Settings and
  does not terminate or disable the current app.

The frontend intentionally holds the `Update` object only in memory. No
updater state is added to SQLite and no database migration is reserved for the
updater. Future v13 (`Missing Abstract Intelligence`) and v14 (`Literature
Library`) remain owned by their feature work.

## Artifact and signing contract

One release tag/commit must publish both platform entries with the same
SemVer. For the current targets:

- macOS arm64: signed `CowPaper.app.tar.gz` and its `.sig`. The app bundle is
  distributed/installed as the update payload; the DMG remains the user-facing
  installer.
- Windows x64: signed NSIS updater artifact (`.nsis.zip` and `.sig`) or the
  corresponding signed MSI updater artifact. The normal `.exe`/`.msi` is the
  installable bundle; the updater uses the generated signed payload.
- `latest.json` must contain both `darwin-aarch64` and `windows-x86_64` entries,
  each with a URL and the exact signature text generated for that artifact.

The private Tauri signing key is never committed. Generate it once with the
Tauri CLI, store the private key (and password, if used) in protected GitHub
Actions secrets, and expose it to release builds only through
`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The public
key may be committed and is the trust anchor shipped in the app. Losing or
rotating the private key without a migration plan prevents existing installs
from accepting future updates.

The existing beta workflows deliberately override
`createUpdaterArtifacts` to `false`, because beta builds are currently
unsigned. `release-updater.yml` enables the base setting, builds both
platforms from the same tag SHA in sequence, signs both artifacts, and lets
the official action generate/upload `latest.json`. It does not run until a
future `v*` tag is pushed. This branch does not alter the v0.1.2 or v0.1.3
tag/release.

## User data locations and invariant

The app resolves one user data directory with Tauri's `app_data_dir()` and
never derives it from the installed executable location:

- macOS: `~/Library/Application Support/com.cowpaper.app/`
- Windows: `%APPDATA%\\com.cowpaper.app\\` (normally
  `C:\\Users\\<user>\\AppData\\Roaming\\com.cowpaper.app\\`)

Persistent data currently includes:

- SQLite: `<app_data_dir>/cowpaper.db`, including Settings in `app_state`,
  subscriptions, recommendation history, `chinese_title`, and AI analysis.
- DeepSeek API key: `<app_data_dir>/secrets.json`, protected as a local file
  with atomic replacement; it is not stored in SQLite or frontend storage.
- Frontend model preference: WebView `localStorage` key `cowpaper_model`.
- Future Library tables/data: the same SQLite file and its normal migrations.

The preservation invariant is:

> Updating the app bundle or running the Windows installer upgrade must never
> delete, recreate, or relocate the user data directory.

Replacing a macOS `.app` bundle leaves Application Support untouched. A normal
Windows NSIS/MSI upgrade replaces installed program files and leaves `%APPDATA%`
untouched. The updater has no code that deletes user data. Database startup
continues to run the existing transactional migrations, so an upgrade may
apply a pending schema migration but must not overwrite existing user values.

## Verification matrix

Automated Rust tests cover the local reopen contract: DB settings, AI output,
translated title, recommendation history, and the secret file survive closing
and reopening the same data directory. Existing migration tests cover schema
upgrade/backfill preservation and idempotence.

The release QA checklist must additionally exercise:

| Case | Expected result |
| --- | --- |
| old version → new version | DB and all existing records remain |
| upgrade with pending schema migration | migration succeeds; existing values remain |
| Settings/API key | settings remain; secret file remains readable |
| recommendation history / Library data | historical rows and later Library rows remain |
| update endpoint/network failure | current CowPaper remains usable |
| invalid signature | update is refused; current CowPaper remains usable |
| no newer version | Settings shows current version/up-to-date |
| macOS + Windows | same SemVer/candidate SHA; platform-specific artifact selected |
