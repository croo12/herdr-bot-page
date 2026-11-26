import { readdirSync } from "node:fs";

const root = "renderer/src";
const layers = ["app", "pages", "features", "shared"];
const fromRenderer = { path: "^renderer/src/" };

function forbid(name, from, to) {
  return { name, from, to, severity: "error" };
}

const sliceRules = [];
for (const layer of ["pages", "features"]) {
  const directory = `${root}/${layer}`;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const name = entry.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const slice = `^${directory}/${name}/`;
    sliceRules.push(
      forbid(`${layer}/${entry.name}: use index.tsx`,
        { ...fromRenderer, pathNot: slice }, { path: slice, pathNot: `${slice}index\\.tsx$` }),
      forbid(`${layer}/${entry.name}: no sibling slice imports`,
        { path: `^${directory}/`, pathNot: slice }, { path: slice }),
    );
  }
}

export default {
  forbidden: [
    ...layers.slice(1).map((layer, index) => forbid(
      `${layer}: no upward imports`,
      { path: `^${root}/${layer}/` },
      { path: `^${root}/(${layers.slice(0, index + 1).join("|")})/` },
    )),
    ...sliceRules,
    forbid("use the desktop bridge for Tauri", {
      ...fromRenderer, pathNot: "^renderer/src/shared/api/desktop/tauri-bridge\\.tsx$",
    }, { path: "(^|/)node_modules/@tauri-apps/api/" }),
    forbid("resolve every renderer import", fromRenderer, { couldNotResolve: true }),
    forbid("no renderer import cycles", fromRenderer, { path: fromRenderer.path, circular: true }),
  ],
  options: {
    parser: "swc",
    doNotFollow: { path: "(^|/)node_modules/" },
    tsConfig: { fileName: "renderer/tsconfig.json" },
    enhancedResolveOptions: { conditionNames: ["types", "import", "browser", "default"] },
  },
};
