# ETS2 Coach Volume Controller

Euro Truck Simulator 2의 코치 DLC 사운드 볼륨을 조절하는 Windows용 도구입니다. 게임 설치본을 읽어 항목별 dB를 정하고, 그 결과로 모드(`.scs`)를 만듭니다. 원본 게임 아카이브는 수정하지 않습니다.

SCS Software가 만든 공식 도구가 아닙니다. Euro Truck Simulator 2는 SCS Software의 상표입니다. 이 프로젝트는 SCS Software와 관련이 없습니다.

## 필요한 것

- Windows
- 본인이 소유한 Euro Truck Simulator 2와 코치 DLC (`dlc_bus.scs`가 있는 설치)
- 직접 빌드할 때: Node.js, Rust stable

## 사용

1. 게임 폴더를 지정합니다. 코치 DLC가 없으면 열리지 않습니다.
2. 슬라이더로 볼륨을 조절합니다. 전체 일괄 값은 모든 항목에 더해집니다.
3. **모드 만들기**로 `.scs`를 모드 폴더에 저장합니다. 기본 위치는 `문서\Euro Truck Simulator 2\mod`입니다.
4. 게임 모드 매니저에서 그 모드를 켭니다.
5. 게임이 업데이트되면 툴에서 모드를 다시 만듭니다.

프리셋과 마지막 화면 값은 앱 데이터 폴더에 저장됩니다. 설정만 담긴 JSON이나, 이 툴이 만든 모드에서 값을 다시 불러올 수 있습니다.

## 만든 모드는 공유하지 마세요

**모드 만들기**가 쓰는 `.scs`에는 코치 DLC의 사운드 뱅크가 들어 있습니다. 볼륨 값만 바뀌고, 음원 자체는 게임 파일과 같습니다. 그 파일은 본인 게임에서만 쓰세요.

다음 장소에는 올리지 마세요.

- Steam 창작마당
- SCS 포럼
- GitHub를 포함한 파일 공유 사이트

이 저장소에도 게임 파일(`.scs`, `.bank`)과 생성된 모드를 넣지 마세요. `.gitignore`가 그 확장자를 무시합니다.

## 빌드

```powershell
npm install
npm run dev
npm run build
```

설치 파일은 `src-tauri/target/release/bundle/nsis/`에 만들어집니다.

## 라이선스

이 프로그램은 [MIT License](LICENSE)입니다. 보증은 없고, 프로그램을 써서 생긴 손해에 대해 저작권자는 책임지지 않는다는 조항이 들어 있습니다. 사본을 배포할 때는 그 고지와 라이선스 문구를 함께 두어야 합니다.

의존성(Tauri 등)은 각자 라이선스를 따르며, 대부분 MIT 또는 Apache-2.0입니다. 이 라이선스는 게임 파일과 생성된 모드에는 미치지 않습니다. 그 파일의 권리는 SCS Software에 있습니다.

---

## English

Unofficial Windows tool that sets per-sound volume for the Euro Truck Simulator 2 coach DLC and writes a local mod. It does not modify the game's own archives.

Euro Truck Simulator 2 is a trademark of SCS Software. This project is not affiliated with SCS Software.

You need your own copy of the game and the coach DLC (`dlc_bus.scs`). Point the tool at the install folder, set the sliders, press **Build mod**, then enable the mod in the game. After a game update, build the mod again.

The `.scs` file this tool writes contains coach DLC sound banks with only the volume values changed. Use that file only in your own game. Do not upload it to the Steam Workshop, the SCS forum, GitHub, or any other file host. Do not commit game archives (`.scs`, `.bank`) or generated mods to this repository.

```powershell
npm install
npm run dev
npm run build
```

The installer is written to `src-tauri/target/release/bundle/nsis/`.

This program is released under the [MIT License](LICENSE): no warranty, and the copyright holder is not liable for damages arising from its use. Copies must keep the copyright notice and license text.

Third-party dependencies keep their own licenses, mostly MIT or Apache-2.0. This license does not cover the game files or the mods this tool writes. Those remain the property of SCS Software.
