// Closed sets of region codes and size presets offered to users, mirroring
// `core::remote_provider::REGIONS` / `SIZE_PRESETS` in src-tauri. Keeping
// these as a literal list (rather than reading them from the provider) means
// an Edge Function can validate a request before ever calling the vendor.

export const REGION_CODES = ["us_east", "us_west", "eu_west", "ap_southeast"] as const;
export type RegionCode = (typeof REGION_CODES)[number];

export const SIZE_PRESETS = ["small", "medium", "large"] as const;
export type SizePreset = (typeof SIZE_PRESETS)[number];

export function isRegionCode(value: unknown): value is RegionCode {
  return typeof value === "string" && (REGION_CODES as readonly string[]).includes(value);
}

export function isSizePreset(value: unknown): value is SizePreset {
  return typeof value === "string" && (SIZE_PRESETS as readonly string[]).includes(value);
}
