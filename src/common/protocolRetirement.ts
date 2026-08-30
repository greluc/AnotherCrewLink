// What a client shows when the server has stopped speaking its protocol version.
//
// A client that has not updated must be told why it stopped working, in the app and in its
// own language, rather than having its socket closed on it. That is the difference between
// a sunset and an outage.
//
// THE ORDERING IS THE WHOLE OF THE DIFFICULTY, AND IT IS NOT SMALL.
//
// A client can only translate a string it already has. The server sends the sentinel below;
// a client that predates this file receives it and shows it verbatim, because
// `socket.on('error')` does `setError(error.message)` and always has.
//
// So this has to be in the fleet's hands *before* the server starts sending it — which
// means shipping it in an ordinary release, and waiting for that release to reach people,
// before the switch-off. Ship the two together and everybody sees the raw sentinel:
// technically a message, practically an outage with a serial number.
//
// That is why the sentinel is also readable English. If the ordering is got wrong anyway,
// what a user sees is still a sentence that tells them what to do.

/// What the server sends instead of accepting a 1.x handshake.
///
/// Both a machine-readable marker and a sentence, for the reason above: a client that has
/// this file translates it, and a client that does not shows it as it stands.
export const PROTOCOL_RETIRED = 'PROTOCOL_RETIRED: this version is no longer supported, please update';

/// Turns a server error into what the user should see.
///
/// Anything that is not the retirement sentinel is passed through untouched. The server
/// sends real errors too, and replacing one of those with a translated guess would hide the
/// thing the user needs to read.
export function retirementMessage(serverMessage: string, translate: (key: string) => string): string {
	if (!serverMessage.startsWith('PROTOCOL_RETIRED')) {
		return serverMessage;
	}
	const translated = translate('game.error_retired');
	// i18next returns the key when nothing has it, which is worse than the English sentence
	// the server sent. A locale that has not been translated yet falls back to English
	// anyway; this covers the case where the key is missing entirely.
	return translated === 'game.error_retired' ? serverMessage : translated;
}
