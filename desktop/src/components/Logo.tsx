import React from "react";
import Image from "next/image";

interface LogoProps {
    isCollapsed: boolean;
}

const Logo = React.forwardRef<HTMLButtonElement, LogoProps>(({ isCollapsed }, _ref) => {
  return (
    isCollapsed ? (
      <div className="flex items-center justify-start mb-2 bg-transparent border-none p-0">
        <Image src="/logo-collapsed.png" alt="Logo" width={40} height={32} />
      </div>
    ) : (
      <span className="text-lg text-center border rounded-full bg-blue-50 border-white font-semibold text-gray-700 mb-2 block items-center">
        <span>MeetFree</span>
      </span>
    )
  );
});

Logo.displayName = "Logo";

export default Logo;
