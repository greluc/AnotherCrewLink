// This fork reads a process and does not write to one. writeMemory, writeBuffer,
// virtualAllocEx, virtualProtectEx and callFunction were removed from the native module
// on 2026-08-25, together with the shellcode helper callFunction was built on: the only
// consumer opens its target with PROCESS_VM_READ, so none of them could have succeeded,
// and writeBuffer reported success regardless of what the kernel did.
const memoryjs = require('./build/Release/memoryjs');

module.exports = {
  // data type constants
  BYTE: 'byte',
  INT: 'int',
  INT32: 'int32',
  UINT32: 'uint32',
  INT64: 'int64',
  UINT64: 'uint64',
  DWORD: 'dword',
  SHORT: 'short',
  LONG: 'long',
  FLOAT: 'float',
  DOUBLE: 'double',
  BOOL: 'bool',
  BOOLEAN: 'boolean',
  PTR: 'ptr',
  POINTER: 'pointer',
  STR: 'str',
  STRING: 'string',
  VEC3: 'vec3',
  VECTOR3: 'vector3',
  VEC4: 'vec4',
  VECTOR4: 'vector4',

  // signature type constants
  NORMAL: 0x0,
  READ: 0x1,
  SUBTRACT: 0x2,

  openProcess(processIdentifier, callback) {
    if (arguments.length === 1) {
      return memoryjs.openProcess(processIdentifier);
    }

    memoryjs.openProcess(processIdentifier, callback);
  },

  getProcesses(callback) {
    if (arguments.length === 0) {
      return memoryjs.getProcesses();
    }

    memoryjs.getProcesses(callback);
  },

  findModule(moduleName, processId, callback) {
    if (arguments.length === 2) {
      return memoryjs.findModule(moduleName, processId);
    }

    memoryjs.findModule(moduleName, processId, callback);
  },

  readMemory(handle, address, dataType, callback) {
    if (arguments.length === 3) {
      return memoryjs.readMemory(handle, address, dataType.toLowerCase());
    }

    memoryjs.readMemory(handle, address, dataType.toLowerCase(), callback);
  },

  readBuffer(handle, address, size, callback) {
    if (arguments.length === 3) {
      return memoryjs.readBuffer(handle, address, size);
    }

    memoryjs.readBuffer(handle, address, size, callback);
  },

  getProcessPath(handle) {
      return memoryjs.getProcessPath(handle);
  },

  findPattern(handle, moduleName, signature, signatureType, patternOffset, addressOffset, skip = 0, callback) {
    if (arguments.length === 6 || arguments.length === 7) {
      return memoryjs.findPattern(handle, moduleName, signature, signatureType, patternOffset, addressOffset, skip);
    }

    memoryjs.findPattern(handle, moduleName, signature, signatureType, patternOffset, addressOffset, callback);
  },

  closeProcess: memoryjs.closeProcess, // nop
};
