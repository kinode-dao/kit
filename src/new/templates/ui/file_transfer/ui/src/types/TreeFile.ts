import { TreeItem } from "react-sortable-tree";
import KinoFile from "./KinoFile";

export interface TreeFile extends TreeItem {
  file: KinoFile;
};