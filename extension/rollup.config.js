import resolve from "@rollup/plugin-node-resolve";
import terser from "@rollup/plugin-terser";

export default [
  {
    input: "node_modules/single-file-core/single-file.js",
    output: {
      file: "lib/single-file.js",
      format: "umd",
      name: "singlefile",
    },
    plugins: [resolve(), terser()],
  },
  {
    input: "node_modules/single-file-core/single-file-bootstrap.js",
    output: {
      file: "lib/single-file-bootstrap.js",
      format: "umd",
      name: "singlefileBootstrap",
    },
    plugins: [resolve(), terser()],
  },
  {
    input: "node_modules/single-file-core/single-file-frames.js",
    output: {
      file: "lib/single-file-frames.js",
      format: "umd",
      name: "singlefileFrames",
    },
    plugins: [resolve(), terser()],
  },
  {
    input: "node_modules/single-file-core/single-file-hooks-frames.js",
    output: {
      file: "lib/single-file-hooks-frames.js",
      format: "iife",
    },
    plugins: [resolve(), terser()],
  },
];
