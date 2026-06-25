package com.spencerchang.schemaforge.service;

import com.spencerchang.schemaforge.model.AppConfig;
import org.springframework.stereotype.Service;

@Service
public class ConfigService {

    public AppConfig readConfig() {
        return new AppConfig();
    }

    public AppConfig writeConfig(AppConfig config) {
        return config;
    }
}
