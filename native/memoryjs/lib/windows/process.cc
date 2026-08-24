#include <node.h>
#include <windows.h>
#include <TlHelp32.h>
#include <vector>
#include "process.h"
#include "memoryjs.h"


// The rights this module opens a process with.
//
// PROCESS_ALL_ACCESS until 2026-08-24. That includes the right to write the target's
// memory, allocate executable pages in it and create threads in it -- every one of which
// this process then held for as long as Among Us was running, whether or not anything
// used them. AnotherCrewLink reads; the write path that once justified them wrote a
// version stamp into the game's menu and was removed with them.
//
// PROCESS_QUERY_LIMITED_INFORMATION rather than PROCESS_QUERY_INFORMATION: it is what
// IsWow64Process and QueryFullProcessImageName need, and it is granted in cases where the
// wider right is not.
#define MEMORYJS_READ_RIGHTS (PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION)

process::process() {}
process::~process() {}

using v8::Exception;
using v8::Isolate;
using v8::String;

process::Pair process::openProcess(const char* processName, const char** errorMessage){
  PROCESSENTRY32 process;
  HANDLE handle = NULL;

  // A list of processes (PROCESSENTRY32)
  std::vector<PROCESSENTRY32> processes = getProcesses(errorMessage);

  for (std::vector<PROCESSENTRY32>::size_type i = 0; i != processes.size(); i++) {
    // Check to see if this is the process we want.
    if (!strcmp(processes[i].szExeFile, processName)) {
      handle = OpenProcess(MEMORYJS_READ_RIGHTS, FALSE, processes[i].th32ProcessID);
      process = processes[i];
      break;
    }
  }

  if (handle == NULL) {
    *errorMessage = "unable to find process";
  }

  return {
    handle,
    process,
  };
}

process::Pair process::openProcess(DWORD processId, const char** errorMessage) {
  PROCESSENTRY32 process;
  HANDLE handle = NULL;

  // A list of processes (PROCESSENTRY32)
  std::vector<PROCESSENTRY32> processes = getProcesses(errorMessage);

  for (std::vector<PROCESSENTRY32>::size_type i = 0; i != processes.size(); i++) {
    // Check to see if this is the process we want.
    if (processId == processes[i].th32ProcessID) {
      handle = OpenProcess(MEMORYJS_READ_RIGHTS, FALSE, processes[i].th32ProcessID);
      process = processes[i];
      break;
    }
  }

  if (handle == NULL) {
    *errorMessage = "unable to find process";
  }

  return {
    handle,
    process,
  };
}

void process::closeProcess(HANDLE hProcess){
  CloseHandle(hProcess);
}

std::vector<PROCESSENTRY32> process::getProcesses(const char** errorMessage) {
  // Take a snapshot of all processes.
  HANDLE hProcessSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, NULL);
  PROCESSENTRY32 pEntry;

  if (hProcessSnapshot == INVALID_HANDLE_VALUE) {
    *errorMessage = "method failed to take snapshot of the process";
  }

  // Before use, set the structure size.
  pEntry.dwSize = sizeof(pEntry);

  // Exit if unable to find the first process.
  if (!Process32First(hProcessSnapshot, &pEntry)) {
    CloseHandle(hProcessSnapshot);
    *errorMessage = "method failed to retrieve the first process";
  }

  std::vector<PROCESSENTRY32> processes;

  // Loop through processes.
  do {
    // Add the process to the vector
    processes.push_back(pEntry);
  } while (Process32Next(hProcessSnapshot, &pEntry));

  CloseHandle(hProcessSnapshot);
  return processes;
}
