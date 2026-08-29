# Windows x64 beta QA

Test each installer on a clean Windows 10 or Windows 11 x64 user profile or VM.

1. Download the `CowPaper_0.1.1_x64-setup.exe` Actions artifact and verify its name.
2. Start the unsigned installer. A Microsoft Defender SmartScreen warning is expected for this beta; do not treat it as a signing failure.
3. Install CowPaper, then confirm the Start Menu, desktop shortcut (if selected), and app window use the CowPaper icon.
4. Launch CowPaper and confirm it creates a fresh local SQLite database for the new Windows user.
5. Add a DeepSeek API key in Settings and restart the app to verify the setting persists.
6. Add a common journal subscription.
7. Manually add a journal with a Print ISSN and/or Online ISSN.
8. Run a manual sync and confirm papers are saved once per DOI.
9. Run a user-initiated AI analysis on eligible papers.
10. Verify Today and History render their expected papers and snapshots.
11. Toggle Favorite and Ignore, then verify their effects after navigation.
12. Run abstract recovery for a paper without an abstract and confirm it does not automatically start AI analysis.
13. Fully exit CowPaper and confirm no background process remains.
14. Relaunch CowPaper and confirm the database, settings, favorites, and history persist.
15. Uninstall from Windows Settings and confirm the installer/app removal behaves as expected for the beta.
