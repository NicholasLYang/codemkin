import React, {useEffect, useState} from 'react';
import allChanges, {ChangeType} from "./changes";
import { createUseStyles } from "react-jss"

const colors: { [s in ChangeType]: string } = {
  same: "#232323",
  add: "green",
  remove: "red"
};

const styles = {
  changeElement: {
    width: "80vw",
    margin: "0"
  },
  changes: {
    display: "flex",
    flexDirection: "column",
    padding: "20px"
  }
} as const;
const useStyles = createUseStyles(styles);

function App() {
  const classes = useStyles();
  const [i, setI] = useState(0);
  useEffect(() => {
    const id = setInterval(() => {
      setI(i => (i + 1) % allChanges.length)
    }, 1000);
    return () => {
      clearInterval(id);
    }
  }, []);
  const changes = allChanges[i];

  return (
    <div className="App">
      <div className={classes.changes}>
      {changes.map(change => {
        return <pre className={classes.changeElement} style={{ color: colors[change.type]}}>{change.content}</pre>
      })}
      </div>
    </div>
  );
}

export default App;
