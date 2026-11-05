import type { Metadata } from "next";
import "./style.css";
export const metadata:Metadata={title:"PANTHEON — Database Workspace",description:"AI-ready database design studio"};
export default function Layout({children}:{children:React.ReactNode}){return <html lang="zh-CN"><body>{children}</body></html>}
