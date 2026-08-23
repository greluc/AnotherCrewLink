import Ajv from 'ajv';
import addFormats from 'ajv-formats';

// Ajv 8 moved format validation into a separate package; the schema below relies on
// the uri format. Until now this file silently used the Ajv 6 that eslint happened
// to pull in, where formats were built in and the option was `format: 'full'`.
const ajv = new Ajv({ allErrors: true });
addFormats(ajv);

export const validateClientPeerConfig = ajv.compile({
	type: 'object',
	properties: {
		forceRelayOnly: {
			type: 'boolean',
		},
		iceServers: {
			type: 'array',
			items: {
				type: 'object',
				properties: {
					urls: {
						type: ['string', 'array'],
						format: 'uri',
						items: {
							type: 'string',
							format: 'uri',
						},
					},
					username: {
						type: 'string',
					},
					credential: {
						type: 'string',
					},
				},
				required: ['urls'],
			},
		},
	},
	required: ['forceRelayOnly', 'iceServers'],
});
