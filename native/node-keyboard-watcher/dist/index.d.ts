import { EventEmitter } from "events";
declare interface KeyboardWatcher {
    on(event: 'keydown', listener: (keyId: number) => void): this;
    on(event: 'keyup', listener: (keyId: number) => void): this;
}
declare class KeyboardWatcher extends EventEmitter {
    constructor();
    addKeyHook(keyId: number): void;
    clearKeyHooks(): void;
    start(): void;
}
export declare const keyboardWatcher: KeyboardWatcher;
export {};
