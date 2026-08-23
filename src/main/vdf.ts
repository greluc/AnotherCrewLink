/**
 * Minimal parser for Valve's KeyValues format, replacing vdf-parser, which was last
 * published in early 2023. Only what Steam's registry.vdf needs: quoted keys, quoted
 * string values, and nested blocks in braces. Comments and unquoted tokens are
 * skipped rather than supported, because Steam does not write them here.
 */

export type VdfValue = string | VdfObject;
export interface VdfObject {
	[key: string]: VdfValue;
}

const TOKEN = /"((?:[^"\\]|\\.)*)"|(\{)|(\})|\/\/[^\n]*/g;

export function parseVdf(input: string): VdfObject {
	const root: VdfObject = {};
	const stack: VdfObject[] = [root];
	let pendingKey: string | null = null;

	TOKEN.lastIndex = 0;
	let match: RegExpExecArray | null;
	while ((match = TOKEN.exec(input)) !== null) {
		const [, quoted, open, close] = match;

		if (quoted !== undefined) {
			const value = quoted.replace(/\\(.)/g, '$1');
			if (pendingKey === null) {
				pendingKey = value;
			} else {
				stack[stack.length - 1][pendingKey] = value;
				pendingKey = null;
			}
		} else if (open !== undefined) {
			// A block always follows a key; a stray one would corrupt the tree.
			if (pendingKey === null) throw new Error('VDF: block without a key');
			const child: VdfObject = {};
			stack[stack.length - 1][pendingKey] = child;
			stack.push(child);
			pendingKey = null;
		} else if (close !== undefined) {
			if (stack.length === 1) throw new Error('VDF: unbalanced closing brace');
			stack.pop();
		}
	}

	if (stack.length !== 1) throw new Error('VDF: unterminated block');
	return root;
}
