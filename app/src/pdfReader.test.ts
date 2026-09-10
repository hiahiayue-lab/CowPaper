import assert from "node:assert/strict";
import {
  detectReaderPlatform,
  isValidReaderApplicationPath,
  readerApplicationName,
  readerPickerOptions,
} from "./pdfReader.ts";

assert.equal(detectReaderPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)", "MacIntel"), "macos");
assert.equal(detectReaderPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32"), "windows");
assert.equal(detectReaderPlatform("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64"), "other");

assert.deepEqual(readerPickerOptions("macos").filters?.[0].extensions, ["app"]);
assert.deepEqual(readerPickerOptions("windows").filters?.[0].extensions, ["exe"]);
assert.equal(readerPickerOptions("other").filters, undefined);

assert.equal(isValidReaderApplicationPath("/Applications/Preview.app", "macos"), true);
assert.equal(isValidReaderApplicationPath("/Applications/Preview", "macos"), false);
assert.equal(isValidReaderApplicationPath("C:\\Program Files\\SumatraPDF\\SumatraPDF.exe", "windows"), true);
assert.equal(isValidReaderApplicationPath("C:\\Program Files\\SumatraPDF\\SumatraPDF", "windows"), false);
assert.equal(isValidReaderApplicationPath("preview; rm -rf /", "other"), false);
assert.equal(readerApplicationName("/Applications/Preview.app"), "Preview");
assert.equal(readerApplicationName("C:\\Program Files\\Reader\\Reader.exe"), "Reader");

console.log("pdf reader tests passed");
