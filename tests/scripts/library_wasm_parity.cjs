'use strict';

// Actual generated wasm-bindgen Node host. Cash and Decimal inputs stay text.
// Full native envelopes establish transport parity, never a financial oracle.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const path = require('node:path');

const [modulePath, parityPath, outputPath] = process.argv.slice(2);
assert(modulePath && parityPath && outputPath, 'module, full native parity and output required');
const wasm = require(path.resolve(modulePath));
const source = JSON.parse(fs.readFileSync(parityPath, 'utf8'));
assert.equal(source.certified, false);
assert.equal(typeof wasm.library, 'function');
const answers = source.cases.map(c => ({
    id: c.id,
    envelope: JSON.parse(wasm.library(c.request_json || JSON.stringify(c.request))),
}));
fs.writeFileSync(outputPath, JSON.stringify({certified: false, cases: answers}) + '\n');
for (let i = 0; i < source.cases.length; i++) {
    assert.deepEqual(answers[i].envelope, source.cases[i].expected_envelope,
        `${source.cases[i].id}: actual wasm32/JavaScript envelope differs`);
}
console.log(JSON.stringify({status: 'passed', complete_transport_requests: answers.length,
    actual_wasm32: true, actual_node_host: true, node: process.version, certified: false}));
