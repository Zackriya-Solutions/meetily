'use client'

import { useEffect } from "react"

import {
    getPreferredUiLanguage,
    setUiLanguage
} from '@/i18n';

export function LanguageInitializer() {
    useEffect(()=> {
        void setUiLanguage(getPreferredUiLanguage());
    },[]);

    return null;
}