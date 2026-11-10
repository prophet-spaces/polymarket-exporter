import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const dashboard = JSON.parse(readFileSync(new URL('./dashboards/wallet.json', import.meta.url)));
const panel = dashboard.panels.find(panel => panel.id === 105);
const query = (result = '>= -Inf') => panel.targets[0].expr
  .replaceAll('${__from:date:seconds}', '600')
  .replaceAll('${__to:date:seconds}', '1200')
  .replaceAll('$wallet', 'test-wallet')
  .replaceAll('${result:raw}', result);
assert(!query().includes('$'), 'unresolved dashboard variable');
assert.equal(dashboard.panels.length, 7);
assert.deepEqual(dashboard.templating.list.map(variable => variable.name), ['wallet', 'result']);

const scope = 'job="polymarket-wallet",instance="test-wallet",wallet="test-wallet"';
const labels = id => `${scope},event_id="${id}",event_slug="event-${id}"`;
const series = (name, labels, values) => ({ series: `${name}{${labels}}`, values });
function event(id, realized, unrealized, total, first = '100+0x4', status = '2 2 2 2 0', activity = '100+0x4') {
  return [
    ...Object.entries({ realized, unrealized, total }).map(([component, values]) =>
      series('polymarket_wallet_event_pnl_usdc', `${labels(id)},component="${component}"`, values)),
    series('polymarket_wallet_event_first_entry_timestamp_seconds', labels(id), first),
    series('polymarket_wallet_event_status', labels(id), status),
    series('polymarket_wallet_event_last_activity_timestamp_seconds', labels(id), activity),
  ];
}
const book = [
  ...event('a', '100 100 120 130 150', '20 20 40 10 0', '120 120 160 140 150', '100+0x4', '2 2 2 2 0', '100 100 500 800 1100'),
  ...event('b', '_ _ _ 30 30', '_ _ _ 0 0', '_ _ _ 30 30', '_ _ _ 720 720', '_ _ _ 0 0', '_ _ _ 1100 1100'),
  ...event('c', '0+0x4', '0 0 0 -5 -15', '0 0 0 -5 -15', '100+0x4', '2+0x4'),
  ...event('d', '0+0x4', '0+0x4', '0+0x4', '100+0x4', '0+0x4'),
  ...event('f', '0+0x4', '0+0x4', '0+0x4', '100+0x4', '2+0x4'),
];
const up = values => series('up', 'job="polymarket-wallet",instance="test-wallet"', values);
const snapshot = series('polymarket_wallet_pnl_snapshot_complete', scope, '1+0x4');
const test = (expr, exp_samples) => ({ expr, eval_time: '20m', exp_samples });
const count = value => [{ labels: '{}', value }];
function rows(ids) {
  const values = { a: [-10, 30, -40, 0], b: [30, 30, 0, 0], c: [-15, 0, -15, 2], f: [0, 0, 0, 2] };
  return ids.flatMap(id => ['total', 'realized', 'unrealized', 'status', 'impact'].map((component, index) => ({
    labels: `{${labels(id)},component="${component}"}`,
    value: index === 4 ? Math.abs(values[id][0]) : values[id][index],
  })));
}
const cases = [
  ['old closed events are hidden while current holdings and period results remain', book, [test(query(), rows(['a', 'b', 'c', 'f'])), test(query('> 0'), rows(['b'])), test(query('< 0'), rows(['a', 'c']))]],
  ['missing baselines stay visible for current holdings even with a loss filter', [...book, ...event('e', '_ _ _ 80 80', '_ _ _ 0 0', '_ _ _ 80 80', '_ _ _ 100 100', '_ _ _ 2 2')], [test(`count(${query()})`, count(24)), test(`count(${query('< 0')})`, count(14))]],
  ['unknown acquisition time cannot imply a zero baseline', event('e', '_ _ _ 80 80', '_ _ _ 0 0', '_ _ _ 80 80', '_ _ _ 0 0', '_ _ _ 2 2'), [test(`count((${query()}) != (${query()}))`, count(3))]],
  ['old closed lifetime profits are not period gains', event('g', '100+0x4', '0+0x4', '100+0x4', '100+0x4', '0+0x4'), [test(query(), [])]],
  ['break-even event opened and closed inside the range remains visible', event('g', '_ _ _ 0 0', '_ _ _ 0 0', '_ _ _ 0 0', '_ _ _ 720 720', '_ _ _ 0 0', '_ _ _ 1100 1100'), [test(`count(${query()})`, count(5)), test(query('> 0'), []), test(query('< 0'), [])]],
  ['closed event P&L corrections remain visible despite old activity time', event('g', '100 100 100 105 110', '0+0x4', '100 100 100 105 110', '100+0x4', '0+0x4'), [test(`count(${query()})`, count(5))]],
  ['disappearing events show missing history instead of a complete zero', event('a', '100 100 100 stale _', '0 0 0 stale _', '100 100 100 stale _', '100 100 100 stale _', '0 0 0 stale _'), [test(`count((${query()}) != (${query()}))`, count(3))]],
  ['empty wallet has no event rows', [], [test(query(), [])]],
];
const suite = { evaluation_interval: '5m', tests: cases.map(([name, inputs, checks]) => ({ name, interval: '5m', input_series: [...inputs, up('1+0x4'), snapshot], promql_expr_test: checks })) };
for (const [name, inputs] of [
  ['failed end scrape cannot show cached results', [...book, up('1 1 1 1 0'), snapshot]],
  ['no scrape history', []],
  ['old exporter without event history', [up('1+0x4')]],
  ['stale scrapes', [...book, up('1 1 _ _ _'), snapshot]],
]) {
  suite.tests.push({ name, interval: '5m', input_series: inputs, promql_expr_test: [test(query(), [])] });
}
suite.tests.push({ name: 'failed start scrape keeps old events unavailable', interval: '5m', input_series: [...book, up('1 1 0 1 1'), snapshot], promql_expr_test: [test(`count(${query()})`, count(17))] });

const temporary = mkdtempSync(join(tmpdir(), 'wallet-pnl-'));
try {
  const file = join(temporary, 'queries.yml');
  writeFileSync(file, JSON.stringify(suite));
  const result = spawnSync('docker', ['run', '--rm', '--entrypoint', 'promtool', '-v', `${file}:/test/queries.yml:ro`, 'prom/prometheus:v3.10.0', 'test', 'rules', '/test/queries.yml'], { stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
