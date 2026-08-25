declare module 'memoryjs' {
	// This module reads another process and nothing else. The write, allocate and
	// call-into-the-target entry points were removed from the native module itself on
	// 2026-08-25, so there is nothing left to declare: the client opens Among Us with
	// PROCESS_VM_READ only, and writeBuffer used to ignore WriteProcessMemory's return
	// value and report success either way, so a call that could never work looked like one
	// that had.
	type Callback<T> = (error: unknown, value: T) => void;

	// Processes

	export interface ProcessObject {
		dwSize: number;
		th32ProcessID: number;
		cntThreads: number;
		th32ParentProcessID: number;
		pcPriClassBase: number;
		szExeFile: string;
		modBaseAddr: number;
		handle: number;
	}

	export function openProcess(identifier: string | number, callback?: Callback<ProcessObject>): ProcessObject;

	export function getProcesses(callback?: Callback<ProcessObject[]>): ProcessObject[];
	export function getProcesses(processId: number, callback?: Callback<ModuleObject[]>): ModuleObject[];

	// Modules

	export interface ModuleObject {
		modBaseAddr: number;
		modBaseSize: number;
		szExePath: string;
		szModule: string;
		th32ProcessID: number;
	}

	export function findModule(identifier: string, processId: number, callback?: Callback<ModuleObject>): ModuleObject;

	// Memory

	export type Vector3 = { x: number; y: number; z: number };
	export type Vector4 = { x: number; y: number; z: number; w: number };
	export type DataType =
		| 'byte'
		| 'int'
		| 'int32'
		| 'uint32'
		| 'int64'
		| 'uint64'
		| 'dword'
		| 'short'
		| 'long'
		| 'float'
		| 'double'
		| 'bool'
		| 'boolean'
		| 'ptr'
		| 'pointer'
		| 'str'
		| 'string'
		| 'vec3'
		| 'vector3'
		| 'vec4'
		| 'vector4';

	export function readMemory<T>(handle: number, address: number, dataType: DataType, callback?: Callback<T>): T;

	export function readBuffer(handle: number, address: number, size: number, callback?: Callback<Buffer>): Buffer;

	export function getProcessPath(handle: number): string;

	export function findPattern(
		handle: number,
		moduleName: string,
		signature: string,
		signatureType: number,
		patternOffset: number,
		addressOffset: number,
		skip: number
	): number;
}
