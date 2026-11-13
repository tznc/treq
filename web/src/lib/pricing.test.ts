import {FAQ_ITEMS, FAQ_SCHEMA} from './pricing';

test('FAQ_SCHEMA mirrors every visible pricing FAQ as a FAQPage', () => {
  expect(FAQ_SCHEMA['@type']).toBe('FAQPage');
  expect(FAQ_SCHEMA.mainEntity.map((q) => [q.name, q.acceptedAnswer.text])).toEqual(FAQ_ITEMS.map((item) => [item.question, item.answer]));
});
