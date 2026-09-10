import {
  detectReaderPlatform,
  isValidReaderApplicationPath,
  readerApplicationName,
  readerPickerOptions,
} from "./pdfReader.ts";

function assert(condition: unknown, message = "assertion failed"): asserts condition {
  if (!condition) throw new Error(message);
}

function deepEqual<T>(actual: T, expected: T, message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

assert(detectReaderPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)", "MacIntel") === "macos");
assert(detectReaderPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32") === "windows");
assert(detectReaderPlatform("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64") === "other");

deepEqual(readerPickerOptions("macos").filters?.[0].extensions, ["app"], "macOS picker extension");
deepEqual(readerPickerOptions("windows").filters?.[0].extensions, ["exe"], "Windows picker extension");
assert(readerPickerOptions("other").filters === undefined);

assert(isValidReaderApplicationPath("/Applications/Preview.app", "macos"));
assert(!isValidReaderApplicationPath("/Applications/Preview", "macos"));
assert(isValidReaderApplicationPath("C:\\Program Files\\SumatraPDF\\SumatraPDF.exe", "windows"));
assert(!isValidReaderApplicationPath("C:\\Program Files\\SumatraPDF\\SumatraPDF", "windows"));
assert(!isValidReaderApplicationPath("preview; rm -rf /", "other"));
assert(readerApplicationName("/Applications/Preview.app") === "Preview");
assert(readerApplicationName("C:\\Program Files\\Reader\\Reader.exe") === "Reader");

console.log("pdf reader tests passed");
