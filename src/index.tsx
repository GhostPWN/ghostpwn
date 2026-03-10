import { render } from "@opentui/solid";
import { InputBar } from "./componants/inputbar";

function App() {
  return (
    <box flexDirection="column" flexGrow={1} backgroundColor="#0a0a0f">
      <InputBar />
    </box>
  );
}

render(() => <App />);
