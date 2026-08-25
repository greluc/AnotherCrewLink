# memoryjs &middot; [![GitHub license](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/Rob--/memoryjs/blob/master/LICENSE.md) [![npm version](https://img.shields.io/npm/v/memoryjs.svg?style=flat)](https://www.npmjs.com/package/memoryjs)

memoryjs is an NPM package for reading and writing process memory! (finally!)

NOTE: version 3 of this library introduces breaking changes that are incompatible with previous versions.
The notable change is that when reading memory, writing memory and pattern scanning you are required to pass the handle
through for the process (that is returned from `memoryjs.openProcess`). This allows for multi-process support.

# Features

- List all open processes
- List all modules associated with a process
- Find a specific module within a process
- Read process memory
- Read buffers from memory
- Fetch a list of memory regions within a process
- Pattern scanning
- Hardware breakpoints (find out what accesses/writes to this address etc)

Functions that this library directly exposes from the WinAPI:
- [ReadProcessMemory](https://docs.microsoft.com/en-us/windows/desktop/api/memoryapi/nf-memoryapi-readprocessmemory)

**This is AnotherCrewLink's vendored fork.** Writing to another process was removed on
2026-08-25: `writeMemory`, `writeBuffer`, `virtualAllocEx`, `virtualProtectEx` and
`callFunction` are gone, along with the shellcode helper `callFunction` was built on. The
only consumer opens its target with `PROCESS_VM_READ`, so none of them could succeed, and
`writeBuffer` ignored `WriteProcessMemory`'s return value and reported success either way.
This fork reads; it does not write.

# Install

This is a Node add-on (last tested to be working on `v8.11.3`) and therefore requires [node-gyp](https://github.com/nodejs/node-gyp) to use.

You may also need to [follow these steps](https://github.com/nodejs/node-gyp#user-content-installation).

`npm install memoryjs`

When using memoryjs, the target process should match the platform architecture of the Node version running.
For example if you want to target a 64 bit process, you should try and use a 64 bit version of Node.

You also need to recompile the library and target the platform you want. Head to the memoryjs node module directory, open up a terminal and to run the compile scripts, type:

`npm run build32` if you want to target 32 bit processes

`npm run build64` if you want to target 64 bit processes

# Node Webkit / Electron

If you are planning to use this module with Node Webkit or Electron, take a look at [Liam Mitchell](https://github.com/LiamKarlMitchell)'s build notes [here](https://github.com/Rob--/memoryjs/issues/23).

# Usage

Initialise:
``` javascript
const memoryjs = require('memoryjs');
const processName = "csgo.exe";
```

### Processes:

Open a process (sync):
``` javascript
const processObject = memoryjs.openProcess(processIdentifier);
```

Open a process (async):
``` javascript
memoryjs.openProcess(processIdentifier, (error, processObject) => {

});
```

Get all processes (sync):
``` javascript
const processes = memoryjs.getProcesses();
```

Get all processes (async):
``` javascript
memoryjs.getProcesses((error, processes) => {

});
```

See the [Documentation](#user-content-process-object) section of this README to see what a process object looks like.

### Modules: 

Find a module (sync):
``` javascript
const module = memoryjs.findModule(moduleName, processId);
```

Find a module (async):
``` javascript
memoryjs.findModule(moduleName, processId, (error, module) => {

});
```

Get all modules (sync):
``` javascript
const modules = memoryjs.getModules(processId);
```

Get all modules (async):
``` javascript
memoryjs.getModules(processId, (error, modules) => {

});
```

See the [Documentation](#user-content-module-object) section of this README to see what a module object looks like.

### Memory:

Read from memory (sync):
``` javascript
const value = memoryjs.readMemory(handle, address, dataType);
```

Read from memory (async):
``` javascript
memoryjs.readMemory(handle, address, dataType, (error, value) => {

});
```

Read buffer from memory (sync):
``` javascript
const buffer = memoryjs.readBuffer(handle, address, size);
```

Read buffer from memory (async):
``` javascript
memoryjs.readBuffer(handle, address, size, (error, buffer) => {

});
```

Fetch memory regions (sync):
``` javascript
const regions = memoryjs.getRegions(handle);
```

Fetch memory regions (async):
``` javascript
memoryjs.getRegions(handle, (regions) => {

});
```

See the [Documentation](#user-content-documentation) section of this README to see what values `dataType` can be.

### Pattern Scanning:

Pattern scanning (sync):
``` javascript
const offset = memoryjs.findPattern(handle, moduleName, signature, signatureType, patternOffset, addressOffset);
```

Pattern scanning (async):
``` javascript
memoryjs.findPattern(handle, moduleName, signature, signatureType, patternOffset, addressOffset, (error, offset) => {

})
```

### Hardware Breakpoints

Attach a debugger:
``` javascript
const success = memoryjs.attatchDebugger(processId, exitOnDetatch);
```

Detatch debugger:
``` javascript
const success = memoryjs.detatchDebugger(processId);
```

Wait for debug devent:
``` javascript
const success = memoryjs.awaitDebugEvent(hardwareRegister, millisTimeout);
```

Handle debug event:
``` javascript
const success = memoryjs.handleDebugEvent(processId, threadId);
```

Set a hardware breakpoint:
``` javascript
const success = memoryjs.setHardwareBreakpoint(processId, address, hardwareRegister, trigger, length);
```

Remove a hardware breakpoint:
``` javascript
const success = memoryjs.removeHardwareBreakpoint(processId, hardwareRegister);
```

# Documentation

Note: this documentation is currently being updated, refer to the [Wiki](https://github.com/Rob--/memoryjs/wiki) for more information.

### Process Object:
``` javascript
{ dwSize: 304,
  th32ProcessID: 10316,
  cntThreads: 47,
  th32ParentProcessID: 7804,
  pcPriClassBase: 8,
  szExeFile: "csgo.exe",
  modBaseAddr: 1673789440,
  handle: 808 }
```

The `handle` and `modBaseAddr` properties are only available when opening a process and not when listing processes.

### Module Object:
``` javascript
{ modBaseAddr: 468123648,
  modBaseSize: 80302080,
  szExePath: 'c:\\program files (x86)\\steam\\steamapps\\common\\counter-strike global offensive\\csgo\\bin\\client.dll',
  szModule: 'client.dll',
  th32ProcessID: 10316 }
  ```

### Data Type:

When using the read functions, the data type (dataType) parameter can either be a string and be one of the following:

`"byte", "int", "int32", "uint32", "int64", "uint64", "dword", "short", "long", "float", "double", "bool", "boolean", "ptr", "pointer", "str", "string", "vec3", "vector3", "vec4", "vector4"`

or can reference constants from within the library:

`memoryjs.BYTE, memoryjs.INT, memoryjs.INT32, memoryjs.UINT32, memoryjs.INT64, memoryjs.UINT64, memoryjs.DWORD, memoryjs.SHORT, memoryjs.LONG, memoryjs.FLOAT, memoryjs.DOUBLE, memoryjs.BOOL, memoryjs.BOOLEAN, memoryjs.PTR, memoryjs.POINTER, memoryjs.STR, memoryjs.STRING, memoryjs.VEC3, memoryjs.VECTOR3, memoryjs.VEC4, memoryjs.VECTOR4`

This is simply used to denote the type of data being read.

Vector3 is a data structure of three floats, Vector4 a data structure of four:

``` javascript
const vector3 = memoryjs.readMemory(handle, address, memoryjs.VEC3); // { x, y, z }
const vector4 = memoryjs.readMemory(handle, address, memoryjs.VEC4); // { w, x, y, z }
```

### Generic Structures:

To read a structure from memory, you will need to read a buffer from memory using the `readBuffer` function, and then you can use the [dissolve](https://github.com/deoxxa/dissolve) library to parse the buffer into a structure.

You don't need to use the library mentioned above, it just makes it easy to turn your buffer into a structure.

### Strings:

You can use this library to read either a "string" or a "char*".

In both cases you want to get the address of the char array:

```c++
std::string str1 = "hello";
std::cout << "Address: 0x" << hex << (DWORD) str1.c_str() << dec << std::endl;

char* str2 = "hello";
std::cout << "Address: 0x" << hex << (DWORD) str2 << dec << std::endl;
```

From here you can simply use this address to write and read memory.

There is one caveat when reading a string in memory however, due to the fact that the library does not know
how long the string is, it will continue reading until it finds the first null-terminator. To prevent an
infinite loop, it will stop reading if it has not found a null-terminator after 1 million characters.

One way to bypass this limitation in the future would be to allow a parameter to let users set the maximum
character count.

### Signature Type:

When pattern scanning, flags need to be raised for the signature types. The signature type parameter needs to be one of the following:

`0x0` or `memoryjs.NORMAL` which denotes a normal signature.

`0x1` or `memoryjs.READ` which will read the memory at the address.

`0x2` or `memoryjs.SUBSTRACT` which will subtract the image base from the address.

To raise multiple flags, use the bitwise OR operator: `memoryjs.READ | memoryjs.SUBTRACT`.

### Hardware Breakpoints:

Hardware breakpoints work by attaching a debugger to the process, setting a breakpoint on a certain address and declaring a trigger type (e.g. breakpoint on writing to the address) and then continuously waiting for a debug event to arise (and then consequently handling it).

This library exposes the main functions, but also includes a wrapper class to simplify the process. For a complete code example, checkout our [debugging example](https://github.com/Rob--/memoryjs/blob/master/examples/debugging.js).

When setting a breakpoint, you are required to pass a trigger type:
- `memoryjs.TRIGGER_ACCESS` - breakpoint occurs when the address is accessed
- `memoryjs.TRIGGER_WRITE` - breakpoint occurs when the address is written to

Do note that when monitoring an address containing a string, the `size` parameter of the `setHardwareBreakpoint` function should be the length of the string. When using the `Debugger` wrapper class, the wrapper will automatically determine the size of the string by attempting to read it.

To summarise:
- When using the `Debugger` class:
  - No need to pass the `size` parameter to `setHardwareBreakpoint`
  - No need to manually pick a hardware register
  - Debug events are picked up via an event listener
  - `setHardwareBreakpoint` returns the register that was used for the breakpoint

- When manually using the debugger functions:
  - The `size` parameter is the size of the variable in memory (e.g. int32 = 4 bytes). For a string, this parameter is the length of the string
  - Manually need to pick a hardware register (via `memoryjs.DR0` through `memoryhs.DR3`). Only 4 hardware registers are available (some CPUs may even has less than 4 available). This means only 4 breakpoints can be set at any given time
  - Need to manually wait for debug and handle debug events
  - `setHardwareBreakpoint` returns a boolean stating whether the operation as successful

For more reading about debugging and hardware breakpoints, checkout the following links:
- [DebugActiveProcess](https://msdn.microsoft.com/en-us/library/windows/desktop/ms679295(v=vs.85).aspx) - attatching the debugger
- [DebugSetProcessKillOnExit](https://docs.microsoft.com/en-us/windows/desktop/api/winbase/nf-winbase-debugsetprocesskillonexit) - kill the process when detatching
- [DebugActiveProcessStop](https://msdn.microsoft.com/en-us/library/windows/desktop/ms679296(v=vs.85).aspx) - detatching the debugger
- [WaitForDebugEvent](https://msdn.microsoft.com/en-us/library/windows/desktop/ms681423(v=vs.85).aspx) - waiting for the breakpoint to be triggered
- [ContinueDebugEvent](https://msdn.microsoft.com/en-us/library/windows/desktop/ms679285(v=vs.85).aspx) - handling the event

#### Using the Debugger Wrapper

The Debugger wrapper contains these functions you should use:

``` javascript
class Debugger {
  attatch(processId, killOnDetatch = false);
  detatch(processId);
  setHardwareBreakpoint(processId, address, trigger, dataType);
  removeHardwareBreakpoint(processId, register);
}
```

1. Attach the debugger
``` javascript
const hardwareDebugger = memoryjs.Debugger;
hardwareDebugger.attach(processId);
```

2. Set a hardware breakpoint
``` javascript
const address = 0xDEADBEEF;
const trigger = memoryjs.TRIGGER_ACCESS;
const dataType = memoryjs.INT;
const register = hardwareDebugger.setHardwareBreakpoint(processId, address, trigger, dataType);
```

3. Create an event listener for debug events (breakpoints)
``` javascript
// `debugEvent` event emission catches debug events from all registers
hardwareDebugger.on('debugEvent', ({ register, event }) => {
  console.log(`Hardware Register ${register} breakpoint`);
  console.log(event);
});

// You can listen to debug events from specific hardware registers
// by listening to whatever register was returned from `setHardwareBreakpoint`
hardwareDebugger.on(register, (event) => {
  console.log(event);
});
```

#### When Manually Debugging

1. Attatch the debugger
``` javascript
const hardwareDebugger = memoryjs.Debugger;
hardwareDebugger.attach(processId);
```

2. Set a hardware breakpoint (determine which register to use and the size of the data type)
``` javascript
// available registers: DR0 through DR3
const register = memoryjs.DR0;
// int = 4 bytes
const size = 4;

const address = 0xDEADBEEF;
const trigger = memoryjs.TRIGGER_ACCESS;
const dataType = memoryjs.INT;

const success = memoryjs.setHardwareBreakpoint(processId, address, register, trigger, size);
```

3. Create the await/handle debug event loop
``` javascript
const timeout = 100;

setInterval(() => {
  // `debugEvent` can be null if no event occurred
  const debugEvent = memoryjs.awaitDebugEvent(register, timeout);

  // If a breakpoint occurred, handle it
  if (debugEvent) {
    memoryjs.handleDebugEvent(debugEvent.processId, debugEvent.threadId);
  }
}, timeout);
```

Note: a loop is not required, e.g. no loop required if you want to simply wait until the first detection of the address being accessed or written to.

