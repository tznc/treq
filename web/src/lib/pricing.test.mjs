import test from 'node:test';
import assert from 'node:assert/strict';
import {FAQ_ITEMS, FAQ_SCHEMA} from './pricing.ts';

test('FAQ_SCHEMA mirrors every visible pricing FAQ as a FAQPage', () => {
  assert.equal(FAQ_SCHEMA['@type'], 'FAQPage');
  assert.deepEqual(
    FAQ_SCHEMA.mainEntity.map((q) => [q.name, q.acceptedAnswer.text]),
    FAQ_ITEMS.map((item) => [item.question, item.answer]),
  );
});
