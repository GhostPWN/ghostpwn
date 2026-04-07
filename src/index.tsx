import { render } from "@opentui/solid";
import { InputBar } from "./componants/inputbar";
import { getProviderName } from "./ai";

function App() {
  return (
    <box flexDirection="column" flexGrow={1} backgroundColor="#0a0a0f">
      <InputBar />
      <box paddingX={2}>
        <text fg="#444444">{getProviderName()}</text>
      </box>
    </box>
  );
}

render(() => <App />);
