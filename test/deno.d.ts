// Under Node, tsc knows neither Deno's globals nor its `npm:` specifiers. The
// supabase/functions code these tests import runs on Deno, as do the
// scripts/*.ts that call Deno, so this declares the slice of Deno they use.
// Extend it when they use more.

declare namespace Deno {
  const args: string[];
  const env: { get(key: string): string | undefined };
  function exit(code?: number): never;
  function readTextFile(path: string | URL): Promise<string>;
  function writeTextFile(path: string | URL, data: string): Promise<void>;
}

interface ImportMeta {
  /** True when this module is the program's entry point. */
  readonly main: boolean;
}

// supabase/functions imports only types from it, at the version its
// package.json pins; the root package.json installs the same version.
declare module "npm:@supabase/supabase-js@2.95.3" {
  export * from "@supabase/supabase-js";
}
