type WebDevice = "hmd" | "left" | "right" | { tracker: string };

interface WebStatus {
  panel: number;
  runtime: "openvr" | "openxr" | "novr";
  attached: WebDevice | null;
  locked: boolean;
  trackers: string[];
  menu: boolean;
  grabbed: boolean;
  resolution: [number, number];
  size: [number, number];
}

interface Window {
  nyaw: {
    command(command: "detach" | "close" | { attach: WebDevice } | { lock: boolean } | { resize: { width: number; height: number } }): Promise<void>;
    status(): Promise<WebStatus>;
    spawn(page: string): Promise<void>;
    onStatus(callback: (status: WebStatus) => void): () => void;
  };
}
