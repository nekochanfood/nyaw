// UI 層のブリッジ（crates/ui/src/web/bridge.js）が注入する `window.nyaw` の型と React 用のフック。
// カーネルとの対話は Tauri のコマンド（command / status / spawn）とイベント（onStatus）。
import { useEffect, useState } from "react";

export type Device = "hmd" | "left" | "right" | { tracker: string };
export type PointerMode = "mouse" | "touch";

export type Command =
  | "detach"
  | "close"
  | { attach: Device }
  | { lock: boolean }
  | { animated: boolean }
  | { pointer_mode: PointerMode }
  | { resize: { width: number; height: number } };

/** パネルの状態（Rust の `WebStatus`）。 */
export interface Status {
  panel: number;
  runtime: "openvr" | "openxr" | "novr";
  pointer_mode: PointerMode;
  attached: Device | null;
  locked: boolean;
  animated: boolean;
  trackers: string[];
  menu: boolean;
  grabbed: boolean;
  /** UI の論理ピクセル（= CSS px）。 */
  resolution: [number, number];
  /** VR 上の大きさ（m）。 */
  size: [number, number];
}

export interface Nyaw {
  command(command: Command): Promise<void>;
  status(): Promise<Status>;
  spawn(page: string): Promise<void>;
  /** 戻り値で購読をやめる。 */
  onStatus(cb: (status: Status) => void): () => void;
}

declare global {
  interface Window {
    nyaw?: Nyaw;
  }
}

/** Tauri の外（`npm run dev` のブラウザ）では何もしない。 */
const noop: Nyaw = {
  command: async () => {},
  status: () => Promise.reject(new Error("Tauri の外")),
  spawn: async () => {},
  onStatus: () => () => {},
};

export const nyaw = (): Nyaw => window.nyaw ?? noop;

/** パネルの状態。起動直後はまだ無いことがあるので、取れるまで問い合わせる。 */
export function useStatus(): Status | null {
  const [status, setStatus] = useState<Status | null>(null);
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const first = () =>
      nyaw()
        .status()
        .then(
          (s) => alive && setStatus(s),
          () => {
            if (alive) timer = window.setTimeout(first, 100);
          },
        );
    first();
    const off = nyaw().onStatus(setStatus);
    return () => {
      alive = false;
      window.clearTimeout(timer);
      off();
    };
  }, []);
  return status;
}
