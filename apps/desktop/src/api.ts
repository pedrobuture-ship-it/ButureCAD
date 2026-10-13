import { invoke } from "@tauri-apps/api/core";

/** Mirrors `CoreInfo` in src-tauri/src/lib.rs. */
export interface CoreInfo {
  name: string;
  version: string;
}

export const coreInfo = (): Promise<CoreInfo> => invoke<CoreInfo>("core_info");
