export type ReaderPlatform = "macos" | "windows" | "other";

export interface ReaderPickerOptions {
  multiple: false;
  directory: false;
  filters?: Array<{ name: string; extensions: string[] }>;
}

/** Keep platform detection injectable so the native picker policy is testable. */
export function detectReaderPlatform(userAgent = "", platform = ""): ReaderPlatform {
  const value = `${userAgent} ${platform}`.toLowerCase();
  if (value.includes("mac") || value.includes("darwin")) return "macos";
  if (value.includes("win")) return "windows";
  return "other";
}

/** Native application pickers should only offer application bundles/executables. */
export function readerPickerOptions(platform: ReaderPlatform): ReaderPickerOptions {
  if (platform === "macos") {
    return { multiple: false, directory: false, filters: [{ name: "macOS Application", extensions: ["app"] }] };
  }
  if (platform === "windows") {
    return { multiple: false, directory: false, filters: [{ name: "Windows Application", extensions: ["exe"] }] };
  }
  return { multiple: false, directory: false };
}

function isAbsolutePath(path: string, platform: ReaderPlatform): boolean {
  if (platform === "windows") return /^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\");
  return path.startsWith("/");
}

export function isValidReaderApplicationPath(path: string, platform: ReaderPlatform): boolean {
  const value = path.trim();
  if (!value || !isAbsolutePath(value, platform)) return false;
  if (platform === "macos") return /\.app[\\/]?$/i.test(value);
  if (platform === "windows") return /\.exe$/i.test(value);
  return true;
}

/** The application name is primary UI; the selected path remains secondary detail. */
export function readerApplicationName(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, "");
  const basename = trimmed.split(/[\\/]/).pop() || trimmed;
  return basename.replace(/\.(app|exe)$/i, "") || "Custom application";
}
