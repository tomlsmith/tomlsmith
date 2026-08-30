export interface NativePlatform {
  readonly platform: string;
  readonly arch: string;
  readonly directory: string;
  readonly packageName: string;
  readonly binaryName: "tomlsmith" | "tomlsmith.exe";
}

export const nativePlatforms: readonly NativePlatform[] = [
  {
    platform: "darwin",
    arch: "arm64",
    directory: "darwin-arm64",
    packageName: "@tomlsmith/cli-darwin-arm64",
    binaryName: "tomlsmith",
  },
  {
    platform: "darwin",
    arch: "x64",
    directory: "darwin-x64",
    packageName: "@tomlsmith/cli-darwin-x64",
    binaryName: "tomlsmith",
  },
  {
    platform: "linux",
    arch: "arm64",
    directory: "linux-arm64",
    packageName: "@tomlsmith/cli-linux-arm64",
    binaryName: "tomlsmith",
  },
  {
    platform: "linux",
    arch: "x64",
    directory: "linux-x64",
    packageName: "@tomlsmith/cli-linux-x64",
    binaryName: "tomlsmith",
  },
  {
    platform: "win32",
    arch: "x64",
    directory: "win32-x64",
    packageName: "@tomlsmith/cli-win32-x64",
    binaryName: "tomlsmith.exe",
  },
];

export function findNativePlatform(
  platform: string,
  arch: string,
): NativePlatform | undefined {
  return nativePlatforms.find(
    (candidate) =>
      candidate.platform === platform && candidate.arch === arch,
  );
}

export function supportedPlatformLabels(): string {
  return nativePlatforms
    .map(({ platform, arch }) => `${platform}-${arch}`)
    .join(", ");
}
