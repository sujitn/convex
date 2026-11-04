// Startup admission: the application mounts only on a ready engine. There is
// no mock or fallback calculator; failure renders an error instead.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const babel = require('@babel/core');
const React = require('react');

const source = path.resolve(__dirname, '../src');
const transform = (name) => babel.transformFileSync(path.join(source, name), {
  presets: ['@babel/preset-env', '@babel/preset-react'],
  babelrc: false,
  configFile: false,
}).code;

async function startup(engineFactory) {
  const rendered = [];
  const App = () => null;
  const Boot = () => null;
  const module = { exports: {} };
  vm.runInNewContext(transform('index.js'), {
    require: (name) => {
      if (name === 'react') return React;
      if (name === './App') return { __esModule: true, default: App };
      if (name === './ui/Boot') return { __esModule: true, default: Boot };
      if (name === './engine/client') return { createEngine: () => { throw new Error('real engine requested'); } };
      if (name === './styles/app.css') return {};
      if (name === 'react-dom/client') return { createRoot: () => ({ render: (element) => rendered.push(element) }) };
      throw new Error(`Unexpected startup dependency: ${name}`);
    },
    window: { __CONVEX_NO_AUTOSTART__: true },
    document: { getElementById: () => ({}) },
    console: { log() {}, error() {} },
    module,
    exports: module.exports,
  });
  await module.exports.main({ engineFactory, container: {} });
  return { rendered, App, Boot };
}

function text(element) {
  if (element === null || element === undefined || typeof element === 'boolean') return '';
  if (typeof element === 'string' || typeof element === 'number') return String(element);
  if (Array.isArray(element)) return element.map(text).join('');
  return text(element.props && element.props.children);
}

test('a failed engine load renders an error and never mounts the application', async () => {
  const { rendered, App } = await startup(() => ({ ready: Promise.reject(new Error('missing engine')) }));
  const last = rendered[rendered.length - 1];
  assert.equal(last.props.role, 'alert');
  assert.match(text(last), /Unable to load the analytics engine/);
  assert.match(text(last), /missing engine/);
  assert.ok(!rendered.some((e) => e.type === App));
});

test('an engine factory that throws has the same admission failure', async () => {
  const { rendered, App } = await startup(() => { throw new Error('no wasm'); });
  assert.equal(rendered[rendered.length - 1].props.role, 'alert');
  assert.ok(!rendered.some((e) => e.type === App));
});

test('a ready engine shows the boot screen first, then mounts the application with it', async () => {
  const engine = { ready: Promise.resolve({ state: 'ready' }) };
  const { rendered, App, Boot } = await startup(() => engine);
  assert.equal(rendered[0].type, Boot);
  const last = rendered[rendered.length - 1];
  assert.equal(last.type, App);
  assert.equal(last.props.engine, engine);
});

test('all browser source parses', () => {
  function parse(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const name = path.join(directory, entry.name);
      if (entry.isDirectory()) parse(name);
      else if (/\.(js|jsx)$/.test(name)) transform(path.relative(source, name));
    }
  }
  parse(source);
});

test('no retired surface is reachable from the browser source', () => {
  const all = [];
  (function walk(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const name = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(name);
      else if (/\.(js|jsx)$/.test(name)) all.push(fs.readFileSync(name, 'utf8'));
    }
  }(source));
  const code = all.join('\n');
  // The convenience entry points the library API replaced are not called.
  for (const retired of ['analyze_bond', 'price_from_yield', 'price_from_spread', 'get_cash_flows', 'price_from_g_spread',
    'price_from_benchmark_spread', 'createMockModule']) {
    assert.ok(!code.includes(retired), `${retired} is referenced`);
  }
});
