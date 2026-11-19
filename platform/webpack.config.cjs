const path = require("node:path");

module.exports = {
  entry: "./src/renderer/index.tsx",
  target: "web",
  output: {
    path: path.join(__dirname, "build", "renderer"),
    filename: "workbench.js",
  },
  resolve: { extensions: [".tsx", ".ts", ".js"] },
  module: {
    rules: [{
      test: /\.tsx?$/,
      exclude: /node_modules/,
      use: {
        loader: "ts-loader",
        options: { transpileOnly: true, compilerOptions: { module: "ESNext" } },
      },
    }],
  },
  performance: { hints: false },
};
