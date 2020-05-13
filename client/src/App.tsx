import React from 'react';
import Timelapse from "./Timelapse";
import changes from "./changes";

function App() {
  return (
    <div className="App">
      <Timelapse changes={changes}/>
    </div>
  );
}

export default App;
