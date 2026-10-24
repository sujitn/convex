'use strict';

// Execute every original request through actual wasm32, retaining full answers.
// Native outputs are transport expectations, never independent financial values.
const fs = require('node:fs');
const zlib = require('node:zlib');
const crypto = require('node:crypto');
const path = require('node:path');
const assert = require('node:assert/strict');

const [modulePath, indexPath, outputPath] = process.argv.slice(2);
assert(modulePath && indexPath && outputPath);
assert(!fs.existsSync(outputPath), 'refuse replacement of actual host evidence');
fs.mkdirSync(outputPath, {recursive: true});
const wasm = require(path.resolve(modulePath));
const index = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
assert.equal(index.certified, false);
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const seen = new Set();
const corpora = [];
const differences = [];
for (const corpus of index.corpora) {
    assert.equal(path.basename(corpus.file), corpus.file);
    const raw = fs.readFileSync(path.join(path.dirname(indexPath), corpus.file));
    assert.equal(sha(raw), corpus.sha256);
    const rows = zlib.gunzipSync(raw).toString('utf8').trimEnd().split('\n');
    assert.equal(rows.length, corpus.cases);
    const answers = [];
    for (const line of rows) {
        const row = JSON.parse(line);
        assert(!seen.has(row.id), `duplicate request: ${row.id}`);
        seen.add(row.id);
        const actual = wasm.library(row.request_json);
        answers.push(JSON.stringify({id: row.id, actual_envelope_json: actual}));
        try {
            assert.deepStrictEqual(JSON.parse(actual), row.expected_envelope);
        } catch (error) {
            differences.push({id: row.id, source: corpus.source, message: error.message.slice(0, 4000)});
        }
    }
    const bytes = zlib.gzipSync(answers.join('\n') + '\n');
    fs.writeFileSync(path.join(outputPath, corpus.file), bytes);
    corpora.push({file: corpus.file, sha256: sha(bytes), cases: answers.length});
}
assert.equal(seen.size, index.complete_requests);
const summary = {status: differences.length ? 'failed' : 'passed', actual_wasm32: true,
    actual_node_host: true, node: process.version, complete_requests: seen.size,
    exact_matches: seen.size - differences.length, differences, corpora,
    native_index_sha256: sha(fs.readFileSync(indexPath)), certified: false};
fs.writeFileSync(path.join(outputPath, 'summary.json'), JSON.stringify(summary, null, 2) + '\n');
console.log(JSON.stringify({...summary, corpora: corpora.length}));
process.exitCode = differences.length ? 1 : 0;
