import assert from 'node:assert/strict';
import { test } from 'node:test';
import { checkNativeUI } from '../lint-native-ui.mjs';

const valid = {
  'controls.rs': 'fn field_frame() { handle.is_focused(w); Timing::FOCUS; Input::new(s).focus_bordered(false); }',
  'motion.rs': 'fn layout() { app.reduce_motion() }',
  'views.rs': 'motion::layout(card)'
};
test('shared controls and motion are accepted', () => assert.deepEqual(checkNativeUI(valid), []));
test('page-local input states must use the shared controls', () => {
  for (const widget of ['Input', 'Textarea', 'InputState', 'TextareaState']) {
    assert.ok(checkNativeUI({ ...valid, 'forms.rs': widget + ' :: new(cx)' }).some(s => s.startsWith('forms.rs')));
  }
});
test('removing the motion preference or focus frame is detected', () => {
  assert.ok(checkNativeUI({ ...valid, 'controls.rs': '', 'motion.rs': 'fn layout() {}' }).length >= 2);
  assert.ok(checkNativeUI({ ...valid, 'views.rs': 'div()' }).length);
});
