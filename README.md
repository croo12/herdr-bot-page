# herdr-bot

herdr에서 실행하는 에이전트와 채팅하는 데스크톱 앱입니다. 봇 하나와 대화하거나 여러 봇을 방에 초대해 `@멘션`으로 일을 넘길 수 있습니다. Windows 앱은 Tauri 2와 WebView2를 사용하며, 봇 엔진과 CLI는 Rust로 작성했습니다.

## Windows에서 시작하기

1. PowerShell에서 `herdr --version`으로 herdr 0.9 이상이 설치되어 있는지 확인합니다. 사용할 에이전트 CLI(예: Codex, Claude Code)에도 로그인합니다.
2. Windows x64 설치 파일 `herdr-bot_1.3.0_x64-setup.exe`를 실행하고 시작 메뉴에서 **herdr-bot**을 엽니다. 설치 파일에는 앱과 CLI가 함께 들어 있습니다.
3. **새 Bot 만들기**에서 프로바이더와 모델을 고르고 작업 폴더를 선택합니다. Codex 업데이트 안내가 뜨면 **Skip**으로 이번 업데이트를 건너뛸 수 있습니다. 터미널에서 직접 설정해야 할 때는 **herdr에서 열기**를 선택합니다.

여러 봇이 방에서 작업할 때 터미널 권한 요청도 대화에 표시됩니다. 한 번만 승인할 수 있는 요청은 같은 방에서 작업 중이지 않은 다른 봇이 검토할 수 있습니다. 검토한 봇은 원래 요청에 답장을 달아 승인·거절 이유를 설명하거나 사용자에게 결정을 맡깁니다. 사용자가 먼저 카드에서 답하면 그 결정이 적용됩니다.

방에서 보낸 내 메시지의 **더 보기 → 작업으로 등록**에서 완료 조건을 입력합니다. 메시지 본문이 작업 목표가 됩니다. 담당 봇은 등록할 때 선택하거나 **미지정**으로 두고 나중에 작업 카드에서 연결할 수 있습니다. 방에 참여한 봇 중 한 명을 선택하며, 실행에 들어가기 전이나 일시 정지한 뒤에 담당자를 바꾸거나 연결을 해제할 수 있습니다.

**담당 봇으로 실행**을 누르면 봇이 한 단계씩 진행하고 원래 메시지에 답장으로 결과를 남깁니다. 봇은 완료 조건을 검사한 근거, 다음 단계, 또는 사용자에게 필요한 입력을 보고합니다. 다음 단계가 있으면 엔진이 자동으로 실행합니다. CLI의 `tasks <방 ID>`에서도 담당 봇과 진행 상태를 조회할 수 있습니다.

봇이 다른 봇이나 방에 일을 위임하면 원래 메시지의 **더 보기 → 스레드 보기**에서 진행 상태와 담당 봇의 답장을 확인할 수 있습니다. 승인이나 입력이 필요할 때 **요청 열기**를 누르면 사용자가 볼 수 있는 요청 카드로 이동합니다. 호스트를 재시작하면 수신함에서 대기 중이던 위임은 다시 처리합니다. 이미 실행을 시작했지만 응답을 확인하지 못한 위임은 중단으로 표시됩니다.

대기 중인 작업은 즉시 일시 정지할 수 있고, 실행 중에는 현재 단계가 끝난 뒤 멈추게 할 수 있습니다. 앱을 다시 열면 대기 중인 작업은 큐에서 이어지고, 실행 중이던 단계는 기존 터미널에서 결과를 기다립니다. 결과 보고 없이 단계가 끝나면 작업 카드에 **확인 필요**가 표시됩니다. 사유와 마지막 진행 기록을 확인한 뒤 **이어서 실행**을 누르세요.

## 1:1 대화와 봇 수신함

사이드바의 **봇**에서 프로필을 선택한 뒤 **1:1 대화 열기**를 누릅니다. 같은 봇을 다시 선택하면 기존 대화가 열립니다. 사용자와 봇의 대화, 두 봇 사이의 대화는 각각 별도 기록을 갖습니다.

봇의 Herdr 패인에서 다음 CLI 명령을 사용할 수 있습니다.

```sh
herdr-bot dm reviewer "이 변경을 검토해 주세요"
herdr-bot inbox
herdr-bot read <chat-id>
```

`inbox`는 해당 패인의 봇에게 온 미처리 메시지를 여러 채팅에서 모아 보여줍니다. 그룹에서는 멘션한 봇이 수신하며, 사용자가 멘션 없이 보낸 글은 현재 멤버 봇들에게 전달됩니다. 앱에서 읽었다고 표시해도 봇의 수신함에는 남습니다.

같은 방에서 A가 `@B`를 멘션해 일을 부탁하면, B는 `say --reply-to <요청 메시지 ID> <방 ID> "확인 결과"`로 답할 수 있습니다. A는 다시 멘션하지 않아도 답글을 한 번 수신합니다. 원래 메시지의 **스레드 보기**에서 후속 답글까지 확인할 수 있습니다.

봇이 대화에 참여하면 그 대화의 메시지를 구독합니다. 수신 대상인 봇은 여러 대화에서 온 메시지를 수신함에서 한 건씩 꺼냅니다. 같은 방의 서로 다른 봇은 동시에 작업할 수 있습니다. 이미 작업 중인 봇에게 다른 방이나 1:1 대화에서 온 메시지는 그 봇의 수신함에서 기다립니다.

답장이나 `pass`를 확정하면 해당 수신 항목이 닫힙니다. 호스트가 실제 작업 도중 중단돼 결과를 확인할 수 없는 항목은 `unconfirmed`로 표시합니다. 실행 결과를 확인한 뒤 봇 패인에서 `herdr-bot inbox --ack <message-id>`로 닫거나 `herdr-bot inbox --retry <message-id>`로 새 턴을 요청할 수 있습니다. 봇끼리 연쇄 응답이 두 단계를 넘으면 다음 메시지는 수신함에 보관하고 자동 실행을 멈춥니다.

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

봇·방 목록, 사용자 정보, 설정, 대화 기록은 TanStack Query가 캐시합니다. 대화 기록은 방별로 과거 페이지를 불러오며, Tauri 이벤트가 도착하면 해당 캐시를 갱신합니다. 이벤트 일련번호가 건너뛰면 열린 화면의 데이터를 다시 조회합니다. 선택한 대화와 작성 중인 초안은 화면의 로컬 상태로 관리합니다.

`renderer/src/app`은 앱 진입점과 전역 프로바이더를, `pages/workspace`는 채팅 화면과 그 상태를 관리합니다. `features/compose-message`와 `features/answer-prompt`는 메시지 작성과 권한·질문 응답을 담당합니다. 페이지와 기능은 각 폴더의 `index.tsx`로 공개합니다. Tauri 통신, 공용 UI와 번역은 `shared`에 있습니다.

## Rust 코드 구조

Rust 엔진(`native/src`)은 다음 여섯 층위를 사용합니다. 표의 위에서 아래로 직접 의존할 수 있으며 중간 계층을 건너뛸 수 있습니다. 예를 들어 `chat.list`는 `controller`에서 `modules/chats`로 바로 연결됩니다. 별개 기능 모듈끼리의 직접 참조와 같은 층위의 `storage ↔ infrastructure` 참조는 신규 의존에서 금지합니다.

| 층위 | 경로 | 이곳에 두는 코드 |
| --- | --- | --- |
| 1. 엔진 조립·실행 | `runtime/` | 엔진 시작과 종료, IPC 리스너 시작, 봇별 수신함 처리와 작업 실행을 관리합니다. |
| 2. 요청 진입점 | `controller/` | Tauri와 CLI에서 받은 요청 이름을 해당 작업에 연결합니다. |
| 3. 유스케이스 | `application/` | 여러 기능을 호출하는 순서를 정합니다. 권한 검토 답글은 검토자 확인 후 승인·거절 적용으로 이어집니다. |
| 4. 기능 | `modules/<기능>/` | 봇, 방, 대화 기록, 작업, 권한 검토 등 각 기능의 규칙을 구현합니다. |
| 5. 기술 연동 | `storage/`, `infrastructure/` | `storage`는 SQLite 연결·스키마·트랜잭션을, `infrastructure`는 Herdr 프로세스·실행 경로·로그를 관리합니다. |
| 6. 공통 도메인 | `domain/` | 여러 기능에서 사용하는 값과 이벤트 등 엔진의 기본 정의를 둡니다. |

새 기능은 `modules/<기능>/`에, 여러 기능에 걸친 작업은 `application/`에 추가합니다. 새 최상위 계층이 필요하면 층위와 책임을 먼저 정하고 README와 검사기의 `LAYERS`를 함께 변경합니다. `lib.rs`는 계층 선언과 공개 API 재노출을 담당하고, `bin/`은 별도 CLI 실행 대상입니다. [계층 검사](native/tests/architecture.rs)

각 기능의 `mod.rs`가 호출 가능한 작업을 공개하고 `service.rs`가 처리 규칙을 구현합니다. `runtime`은 기능에 필요한 저장소, Herdr 세션, 작업 큐 등록 함수를 연결합니다. SQLite 쿼리는 `storage/chats.rs`, `storage/transcript.rs`처럼 기능별 파일에 두고, `Store::tx`가 같은 트랜잭션에 묶인 저장소를 제공합니다. 예를 들어 방에 메시지를 보낼 때 대화 기록 추가와 방 상태 갱신이 함께 커밋된 뒤 변경 이벤트를 발행합니다.

데스크톱 앱의 `desktop/src/controller`와 `desktop/src/attachments`는 Tauri 명령과 시스템 대화상자를 연결하고 첨부파일 처리 및 파일 접근을 담당합니다.

## 모듈 경계 검사

`npm ci`를 실행하면 Husky가 Git 훅을 설치합니다. 커밋마다 `npm run check:pre-commit`이 포맷, 렌더러 린트, 모듈 경계를 검사합니다. 훅은 작업 트리를 읽기만 하며 부분 스테이징한 파일의 나머지 수정도 검사에 포함됩니다. Windows와 macOS의 PR CI도 `npm run check`에서 같은 검사를 실행합니다.

- `npm run fmt`는 렌더러 소스·테스트·Vite 설정의 TypeScript와 TSX를 Oxfmt로, `native`와 `desktop`의 Rust 코드를 rustfmt로 정렬합니다. `npm run fmt:check`로 변경 없이 확인할 수 있습니다.
- `npm run lint`는 Oxlint의 TypeScript, React Hooks, JSX 접근성 규칙을 검사합니다. 복합 위젯은 명시적인 ARIA 역할을 허용합니다. Tauri SDK를 호출할 수 있는 위치는 기존 렌더러 모듈 경계 검사에서 검증합니다.
- `npm run lint:rust`는 엔진·CLI와 Tauri 데스크톱 크레이트에 Clippy를 실행합니다. CI는 데스크톱 번들을 빌드한 뒤 릴리스 설정으로도 Clippy를 실행합니다.

- 렌더러는 `app → pages → features → shared` 방향으로 하위 레이어를 참조할 수 있습니다. 다른 page나 feature 슬라이스의 내부 파일은 외부에 공개하지 않고, 같은 레이어의 서로 다른 슬라이스는 서로 import하지 않습니다. 다른 레이어에서 슬라이스를 사용할 때는 `index.tsx`를 거칩니다. Tauri SDK 호출은 `shared/api/desktop/tauri-bridge.tsx`에 둡니다. 검사기는 alias와 상대 경로를 해석하고 순환 import도 검사합니다. [렌더러 규칙](scripts/architecture/renderer.mjs)
- Rust 기능 모듈은 구현 하위 모듈을 비공개로 두고 필요한 항목을 `mod.rs`에서 재공개합니다. 엔진 검사는 소스에 명시한 import와 함수 본문의 경로로 모듈 간 직접 의존을 수집합니다. 매크로가 생성하는 코드와 인자로 전달한 타입의 메서드 호출은 이 그래프의 범위 밖입니다. Cargo 메타데이터로는 로컬 패키지 의존 방향인 `desktop → native`를 별도로 확인합니다.
- Rust 기능 모듈 사이의 직접 참조는 금지합니다. 봇 연결과 대화 턴 실행처럼 여러 기능에 걸친 흐름은 `application`에서 조합합니다. 계층을 향한 새 의존은 허용 목록에서 검토하며, 마지막 참조를 제거하면 해당 항목도 삭제합니다. [의존 목록](native/tests/architecture/dependencies.txt)

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
