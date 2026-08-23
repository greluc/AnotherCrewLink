{
  "targets": [
    {
      "target_name": "keywatcher",
      "sources": [
        "src/lib/keywatcher.cpp",
        "src/lib/keyhandler.cpp",
        "src/lib/getkeystate.cpp",
      ],
      'cflags!': [ '-fno-exceptions' ],
      'cflags_cc!': [ '-fno-exceptions' ],
      'cflags_cc': [ '-std=c++20' ],
      'xcode_settings': { 'CLANG_CXX_LANGUAGE_STANDARD': 'c++20' },
      'include_dirs': [
        'src/lib',
        "<!@(node -p \"require('node-addon-api').include\")"
      ],
      'dependencies': ["<!(node -p \"require('node-addon-api').gyp\")"],
      'conditions': [
        ['OS=="win"', {
          "msvs_settings": {
            "VCCLCompilerTool": {
              "ExceptionHandling": 1,
              "AdditionalOptions": [ "/std:c++20" ]
            }
          }
        }],
        ['OS=="mac"', {
          "xcode_settings": {
            "CLANG_CXX_LIBRARY": "libc++",
            'GCC_ENABLE_CPP_EXCEPTIONS': 'YES',
            'MACOSX_DEPLOYMENT_TARGET': '10.7'
          }
        }],
        ['OS=="linux"', {
            'link_settings': {
                'libraries': [
                    '-lX11'
                ]
          }
        }]
      ]
    }
  ]
}
