#pragma once
#ifndef MEMORY_H
#define MEMORY_H
#define WIN32_LEAN_AND_MEAN

#include <node.h>
#include <windows.h>
#include <TlHelp32.h>

class memory {
public:
  memory();
  ~memory();
  std::vector<MEMORY_BASIC_INFORMATION> getRegions(HANDLE hProcess);

  template <class dataType>
  dataType readMemory(HANDLE hProcess, DWORD64 address) {
    dataType cRead = dataType();
    ReadProcessMemory(hProcess, (LPVOID)address, &cRead, sizeof(dataType), NULL);
    return cRead;
  }

  char* readBuffer(HANDLE hProcess, DWORD64 address, SIZE_T size) {
    char* buffer = new char[size];
    ReadProcessMemory(hProcess, (LPVOID)address, buffer, size, NULL);
    return buffer;
  }

  char readChar(HANDLE hProcess, DWORD64 address) {
    char value = 0;
    ReadProcessMemory(hProcess, (LPVOID)address, &value, sizeof(char), NULL);
    return value;
	}

};
#endif
#pragma once