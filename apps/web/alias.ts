import { fileURLToPath } from "node:url";

const dir = (path: string) => fileURLToPath(new URL(path, import.meta.url));

/** インポートの別名。tsconfig.json の paths と揃える */
export const alias = [
  { find: /^@\//, replacement: dir("./src/") },
  { find: /^@locale\//, replacement: dir("./locale/") },
  { find: /^@assets\//, replacement: dir("../../assets/") },
];
