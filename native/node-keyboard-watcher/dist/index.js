"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.keyboardWatcher = void 0;
const events_1 = require("events");
const lib = require("bindings")("keywatcher");
class KeyboardWatcher extends events_1.EventEmitter {
    constructor() {
        super();
    }
    addKeyHook(keyId) {
        lib.addKeyHook(keyId);
    }
    clearKeyHooks() {
        lib.clearKeyHooks();
    }
    start() {
        lib.start(this.emit.bind(this));
    }
}
exports.keyboardWatcher = new KeyboardWatcher();
//# sourceMappingURL=index.js.map