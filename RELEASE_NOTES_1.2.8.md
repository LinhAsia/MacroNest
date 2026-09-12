# MacroNest v1.2.8

## New features

- **Background Key Action (BackgroundKey)**: Send keystrokes directly to background applications (games, emulators, chats) without stealing window focus.
- **Multiple Key Modes**: Full support for Press (quick tap), Down (hold down), Up (release), and TypeText (character-by-character text typing).
- **Dynamic Window Dropdown Refresh**: Macro step dropdowns (BackgroundKey, BackgroundClick, Memory) now automatically refresh the list of open windows upon click, instantly detecting apps opened after MacroNest.
- **Synchronized Key Selection UI**: Aligned BackgroundKey keypicker UI with standard KeyPress actions, supporting manual key text, dynamic variables ({var}), physical key capture, and a categorized keypicker menu.

## Improvements

- **Intelligent Child HWND Resolution**: Automatically locates target child input windows (including Chromium/Electron's Chrome_RenderWidgetHostHWND in Discord/Slack/Chrome and native Edit/RichEdit controls).
- **Thread Input Synchronization**: Temporarily attaches thread input during background message dispatch to synchronize keyboard input state with the target thread seamlessly.
- **Robust Window Selector Matching**: Enhanced HWND verification (IsWindow) and base title parsing, keeping target bindings intact even when window titles change dynamically (such as unread notification badges or channel switching).
- **Automatic Character Typing in Background**: Automatically generates and dispatches WM_CHAR messages for printable characters and text controls when operating in the background.

## Bug fixes

- Fixed keystroke duplication/tripling ("Sss" instead of "s") when executing macro actions while the target window is in the foreground.
- Fixed missing window entries in MacroPanel dropdowns when target applications are launched after MacroNest.
- Fixed silent background key dispatch failures caused by strict exact window title matching on dynamic titles.
- Fixed child window discovery skipping valid controls in background and occluded windows.
