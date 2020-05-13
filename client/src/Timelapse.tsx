import React, {useEffect, useState} from "react";
import {ChangeElement, ChangeType} from "./changes";
import {createUseStyles} from "react-jss";

interface Props {
  changes: Array<Array<ChangeElement>>
}

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
  code: {
    display: "flex",
    flexDirection: "column",
    padding: "20px"
  }
} as const;
const useStyles = createUseStyles(styles);

const Timelapse: React.FC<Props> = ({ changes }) => {
  const [i, setI] = useState(0);
  const classes = useStyles();
  useEffect(() => {
    const id = setInterval(() => {
      setI(i => (i + 1) % changes.length)
    }, 1000);
    return () => {
      clearInterval(id);
    }
  }, [changes.length]);
  const changeElements = changes[i];
  return <div className={classes.code}>
    {changeElements.map(elem => {
      return <pre className={classes.changeElement} style={{ color: colors[elem.type]}}>{elem.content}</pre>
    })}
  </div>
}

export default Timelapse;