// Actual component/startup source tests. Loader doubles do not price instruments.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const babel = require('@babel/core');
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');

const source = path.resolve(__dirname, '../src');
const transform = (name) => babel.transformFileSync(path.join(source, name), {
  presets: ['@babel/preset-env', '@babel/preset-react'],
  babelrc: false,
  configFile: false,
}).code;

async function startup(load) {
  const rendered = [];
  const App = () => null;
  vm.runInNewContext(transform('index.js'), {
    require: (name) => {
      if (name === 'react') return React;
      if (name === './App') return App;
      if (name === './styles/bloomberg.css') return {};
      if (name === '../pkg') return load();
      if (name === 'react-dom/client') return {
        createRoot: () => ({ render: (element) => rendered.push(element) }),
      };
      throw new Error(`Unexpected startup dependency: ${name}`);
    },
    document: { getElementById: () => ({}) },
    console: { log() {}, error() {} },
    exports: {},
  });
  await new Promise(setImmediate);
  assert.equal(rendered.length, 1);
  return { element: rendered[0], App };
}

test('missing WASM renders an error and never mounts analytics with fake results', async () => {
  const { element } = await startup(() => { throw new Error('missing engine'); });
  assert.equal(element.type, 'div');
  assert.equal(element.props.role, 'alert');
  assert.equal(element.props.children, 'Unable to load the analytics engine.');
});

test('failed WASM initialization has the same admission failure', async () => {
  let initialized = false;
  const { element } = await startup(() => ({
    __esModule: true,
    default: async () => { initialized = true; throw new Error('initialization failed'); },
  }));
  assert.equal(initialized, true);
  assert.equal(element.type, 'div');
  assert.equal(element.props.role, 'alert');
});

test('a successful loader passes the original engine to the application', async () => {
  const engine = { __esModule: true, default: async () => {} };
  const { element, App } = await startup(() => engine);
  assert.equal(element.type, React.StrictMode);
  assert.equal(element.props.children.type, App);
  assert.equal(element.props.children.props.wasmModule, engine);
});

test('spread display retains zero and never substitutes Z-spread for absent OAS', () => {
  const module = { exports: {} };
  vm.runInNewContext(transform('components/SpreadAnalysis.jsx'), {
    require: () => React, module, exports: module.exports,
  });
  const html = renderToStaticMarkup(React.createElement(module.exports.default, {
    analysis: { z_spread: 0, asw_spread: 12.5, is_callable: true, oas: null },
  }));
  assert.match(html, /value="0"/);
  assert.match(html, /12\.5/);
  assert.match(html, /OAS<\/div><div class="spread-value">---/);
  assert.doesNotMatch(html, /G-Spread|Benchmark|≈ Z-Sprd|~0/);
});

test('all browser source parses after the removed callbacks and panel', () => {
  function parse(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const name = path.join(directory, entry.name);
      if (entry.isDirectory()) parse(name);
      else if (/\.(js|jsx)$/.test(name)) transform(path.relative(source, name));
    }
  }
  parse(source);
});
