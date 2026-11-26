# herdr-bot

herdr에서 실행하는 에이전트와 채팅하는 데스크톱 앱입니다. 봇 하나와 대화하거나 여러 봇을 방에 초대해 `@멘션`으로 일을 넘길 수 있습니다. Windows 앱은 Tauri 2와 WebView2를 사용하며, 봇 엔진과 CLI는 Rust로 작성했습니다.

## Windows에서 시작하기

1. PowerShell에서 `herdr --version`으로 herdr 0.9 이상이 설치되어 있는지 확인합니다. 사용할 에이전트 CLI(예: Codex, Claude Code)에도 로그인합니다.
2. Windows x64 설치 파일 `herdr-bot_1.3.0_x64-setup.exe`를 실행하고 시작 메뉴에서 **herdr-bot**을 엽니다. 설치 파일에는 앱과 CLI가 함께 들어 있습니다.
3. **새 Bot 만들기**에서 프로바이더와 모델을 고르고 작업 폴더를 선택합니다. Codex 업데이트 안내가 뜨면 **Skip**으로 이번 업데이트를 건너뛸 수 있습니다. 터미널에서 직접 설정해야 할 때는 **herdr에서 열기**를 선택합니다.

여러 봇이 방에서 작업할 때 터미널 권한 요청도 대화에 표시됩니다. 한 번만 승인할 수 있는 요청은 같은 방에서 작업 중이지 않은 다른 봇이 검토할 수 있습니다. 검토한 봇은 원래 요청에 답장을 달아 승인·거절 이유를 설명하거나 사용자에게 결정을 맡깁니다. 사용자가 먼저 카드에서 답하면 그 결정이 적용됩니다.

## 소스에서 실행하고 빌드하기

Windows 개발 환경에는 Node.js 24 이상, Rust stable MSVC, Visual Studio Build Tools의 **C++를 사용한 데스크톱 개발**, WebView2가 필요합니다. 저장소 루트에서 실행합니다.

```powershell
npm ci
npm start              # 개발 모드로 실행
npm test               # 화면, Rust 엔진과 CLI 테스트
npm run build          # Windows 설치 파일 생성
```

Windows x64 설치 파일은 `desktop/target/release/bundle/nsis/`에 생성됩니다.

## 화면 코드 구조

렌더러의 TypeScript 소스, 테스트, Vite 설정은 모두 `.tsx`로 작성합니다. `npm test --workspace renderer`로 확장자 규칙과 렌더러 테스트를 확인할 수 있습니다.

`renderer/src/app`은 앱 진입점과 전역 프로바이더를, `pages/workspace`는 채팅 화면과 그 상태를 관리합니다. `features/compose-message`와 `features/answer-prompt`는 메시지 작성과 권한·질문 응답을 담당합니다. 페이지와 기능은 각 폴더의 `index.tsx`로 공개합니다. Tauri 통신, 공용 UI와 번역은 `shared`에 있습니다.

## Rust 코드 구조

Tauri 창과 CLI 모두 `native/src/controller`를 통해 같은 엔진 서비스를 호출합니다. 기능을 변경할 때는 `native/src/modules` 아래의 해당 폴더에서 시작합니다.

| 경로 | 책임 |
| --- | --- |
| `native/src/controller` | 요청 이름을 서비스에 연결하고 CLI의 로컬 IPC 요청을 받습니다. |
| `native/src/modules` | 대화(`chats`), 방(`rooms`), 봇(`bots`), 설정(`settings`), 대화 기록(`transcript`), 작업 실행(`turns`), 터미널 질문(`prompts`), 동료 봇의 권한 검토(`reviews`)를 각각 관리합니다. |
| `native/src/storage` | SQLite 스키마와 트랜잭션을 관리합니다. 변경 이벤트는 커밋이 성공한 뒤 순서대로 발행합니다. |
| `native/src/runtime`, `native/src/infrastructure` | 엔진 수명 주기와 작업 큐는 `runtime`에, herdr 프로세스 연동과 실행 경로 탐색은 `infrastructure`에 있습니다. |
| `desktop/src/controller`, `desktop/src/attachments` | Tauri 명령과 시스템 대화상자를 연결하고, 첨부파일의 처리 규칙과 파일 접근을 분리합니다. |

각 기능의 `mod.rs`가 호출 가능한 작업을 공개하고 `service.rs`가 처리 규칙을 구현합니다. SQLite를 사용하는 기능은 쿼리를 자체 `repository.rs`에 둡니다. 예를 들어 방에 메시지를 보낼 때 대화 기록 추가와 방 상태 갱신은 같은 트랜잭션의 저장소를 사용합니다.

## CLI와 데이터

앱을 한 번 실행하면 `%USERPROFILE%\.herdr-bot\bin\herdr-bot.exe`에서 CLI를 사용할 수 있습니다.

```powershell
$cli = "$env:USERPROFILE\.herdr-bot\bin\herdr-bot.exe"
& $cli status
& $cli bot list
& $cli --help
```

봇, 방, 대화 기록은 `%USERPROFILE%\.herdr-bot\chat.sqlite3`에, 첨부 파일은 같은 폴더의 `attachments`에 저장합니다. 봇의 터미널은 `herdr session attach herdr-bot`으로 열 수 있습니다.

| 환경 변수 | 용도 |
| --- | --- |
| `HERDR_BOT_HOME` | 데이터 폴더 지정 |
| `HERDR_BIN_PATH` | herdr 실행 파일 경로 지정 |
| `HERDR_BOT_SESSION` | 봇이 사용할 herdr 세션 지정. 기본값은 `herdr-bot` |

### 엔진 요청 로그

엔진은 요청이 끝날 때마다 데이터 폴더의 `requests.jsonl`에 JSON 한 줄을 추가합니다. 완료 시각(`timestampMs`, Unix 밀리초), 호출 경로(`desktop` 또는 `ipc`), 요청 이름, 처리 시간(`durationMs`), 성공 여부를 기록합니다. 실패한 요청에는 `errorCode`가 추가됩니다. 메시지 본문과 요청 인자는 로그에서 제외합니다.

`durationMs`는 엔진이 요청을 받은 뒤 응답을 만들기까지의 시간입니다. 예를 들어 `message.send`에서는 메시지 저장과 작업 큐 등록까지 측정하며, 봇이 나중에 답변을 생성하는 시간은 별개입니다. 파일이 5 MiB를 넘기 전에 `requests.1.jsonl`로 교체해 직전 파일 하나를 보관합니다.

기본 데이터 폴더를 사용할 때 PowerShell에서 최근 요청을 실시간으로 볼 수 있습니다.

```powershell
Get-Content "$env:USERPROFILE\.herdr-bot\requests.jsonl" -Tail 10 -Wait
```

앱 소스에는 MIT 라이선스가 적용됩니다. [라이선스](LICENSE) · [프로바이더 아이콘 출처](renderer/public/provider-icons/NOTICE.md)
