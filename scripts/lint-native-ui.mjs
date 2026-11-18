import { readdirSync, readFileSync } from 'node:fs';
import { join, basename } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../crates/shipforge-desktop/src/', import.meta.url));
const inputConstructor = /\b(?:Input|Textarea)(?:State)?\s*::\s*new\s*\(/;

export function checkNativeUI(sources) {
  const errors = [];
  for (const [name, source] of Object.entries(sources)) {
    if (basename(name) !== 'controls.rs' && inputConstructor.test(source)) {
      errors.push(name + ' must use the shared Field and field_frame controls');
    }
  }
  const controls = sources['controls.rs'] || '';
  for (const expression of [/fn\s+field_frame\s*\(/, /Timing::FOCUS/, /is_focused\s*\(/, /focus_bordered\(false\)/]) {
    if (!expression.test(controls)) errors.push('controls.rs is missing the native focus contract: ' + expression);
  }
  const motion = sources['motion.rs'] || '';
  if (!/fn\s+layout\s*\(/.test(motion) || !/reduce_motion\s*\(/.test(motion)) errors.push('motion.rs must handle layout and live reduced motion');
  if (!Object.entries(sources).some(([name, src]) => name !== 'motion.rs' && /motion::layout\s*\(/.test(src))) errors.push('workbench must apply the shared layout motion');
  return errors;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  const sources = Object.fromEntries(readdirSync(root).filter(file => file.endsWith('.rs')).map(file => [file, readFileSync(join(root, file), 'utf8')]));
  const errors = checkNativeUI(sources);
  if (errors.length) {
    console.error(errors.join('\n'));
    process.exitCode = 1;
  } else {
    console.log('Verified native controls and layout motion.');
  }
}
