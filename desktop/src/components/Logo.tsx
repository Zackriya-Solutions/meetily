import React from "react";
import Image from "next/image";

interface LogoProps {
    isCollapsed: boolean;
}

const Logo = React.forwardRef<HTMLButtonElement, LogoProps>(({ isCollapsed }, _ref) => {
  return (
    <div className="relative overflow-hidden transition-all duration-300">
      {isCollapsed ? (
        <div className="flex items-center justify-start mb-2 bg-transparent border-none p-0 animate-fade-in">
          <Image src="/logo-collapsed.png" alt="MeetFree" width={40} height={32} />
        </div>
      ) : (
        <div className="mb-2 animate-fade-in">
          <Image
            src="/logo.png"
            alt="MeetFree"
            width={845}
            height={295}
            className="h-auto w-full max-w-[180px]"
            priority
          />
        </div>
      )}
    </div>
  );
});

Logo.displayName = "Logo";

export default Logo;
