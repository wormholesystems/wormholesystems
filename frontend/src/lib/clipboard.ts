import { toast } from 'svelte-sonner';

/** Put `text` on the clipboard; false (and a toast, unless silent) when the browser refuses. */
export async function copyText(
	text: string,
	opts: { success?: string; description?: string; silent?: boolean } = {},
): Promise<boolean> {
	try {
		await navigator.clipboard.writeText(text);
		if (opts.success) toast.success(opts.success, { description: opts.description });
		return true;
	} catch {
		if (!opts.silent) toast.error('Clipboard access denied');
		return false;
	}
}

/**
 * Put text that is still on its way onto the clipboard, e.g. a name the server is working
 * out. Browsers only allow a write during the click that asked for it, and Safari counts a
 * write after a network round trip as outside it, so the write starts now with the text as
 * a promise where the browser takes one. Resolves to the text once copied, null when the
 * browser refuses or the text never arrives; `failed` is what the toast says then.
 */
export async function copyPending(
	text: Promise<string>,
	opts: { failed?: string } = {},
): Promise<string | null> {
	let textFailed = false;
	const arriving = text.catch((err: unknown) => {
		textFailed = true;
		throw err;
	});
	try {
		if (typeof ClipboardItem !== 'undefined' && navigator.clipboard?.write) {
			const blob = arriving.then((t) => new Blob([t], { type: 'text/plain' }));
			await navigator.clipboard.write([new ClipboardItem({ 'text/plain': blob })]);
			return await arriving;
		}
		const resolved = await arriving;
		await navigator.clipboard.writeText(resolved);
		return resolved;
	} catch {
		toast.error(textFailed ? (opts.failed ?? 'Nothing to copy') : 'Clipboard access denied');
		return null;
	}
}

/** Read the clipboard; null (and a toast) when the browser refuses. */
export async function readText(): Promise<string | null> {
	try {
		if (!navigator.clipboard?.readText) throw new Error('unavailable');
		return await navigator.clipboard.readText();
	} catch {
		toast.error('Clipboard access denied');
		return null;
	}
}
