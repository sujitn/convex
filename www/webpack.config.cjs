const path = require('path');
const HtmlWebpackPlugin = require('html-webpack-plugin');

// The engine (pkg/, built with `wasm-pack --target web`) runs in a Web
// Worker; webpack emits the .wasm as an asset via `new URL(..., import.meta.url)`.
module.exports = (env, argv) => ({
  entry: './src/index.js',
  output: {
    path: path.resolve(__dirname, 'dist'),
    filename: '[name].[contenthash].js',
    assetModuleFilename: '[name].[contenthash][ext]',
    clean: true,
  },
  devtool: argv.mode === 'production' ? false : 'eval-cheap-module-source-map',
  module: {
    rules: [
      {
        test: /\.(js|jsx)$/,
        exclude: /node_modules/,
        resolve: { fullySpecified: false },
        use: {
          loader: 'babel-loader',
          options: { presets: ['@babel/preset-env', '@babel/preset-react'] },
        },
      },
      { test: /\.css$/, use: ['style-loader', 'css-loader'] },
    ],
  },
  resolve: { extensions: ['.js', '.jsx'] },
  plugins: [new HtmlWebpackPlugin({ template: './index.html' })],
  devServer: {
    static: { directory: path.join(__dirname, 'dist') },
    compress: true,
    port: 3000,
    hot: true,
  },
  performance: { hints: false },
});
