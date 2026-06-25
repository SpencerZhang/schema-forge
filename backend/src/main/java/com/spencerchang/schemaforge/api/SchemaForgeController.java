package com.spencerchang.schemaforge.api;

import com.spencerchang.schemaforge.model.AppConfig;
import com.spencerchang.schemaforge.model.GenerateResult;
import com.spencerchang.schemaforge.service.ConfigService;
import com.spencerchang.schemaforge.service.DocumentGenerationService;
import org.springframework.web.bind.annotation.CrossOrigin;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.PutMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@CrossOrigin(origins = {"http://127.0.0.1:1420", "http://localhost:1420"})
@RestController
@RequestMapping("/api")
public class SchemaForgeController {

    private final ConfigService configService;
    private final DocumentGenerationService documentGenerationService;

    public SchemaForgeController(ConfigService configService,
                                 DocumentGenerationService documentGenerationService) {
        this.configService = configService;
        this.documentGenerationService = documentGenerationService;
    }

    @GetMapping("/health")
    public String health() {
        return "ok";
    }

    @GetMapping("/config")
    public AppConfig getConfig() {
        return configService.readConfig();
    }

    @PutMapping("/config")
    public AppConfig saveConfig(@RequestBody AppConfig config) {
        return configService.writeConfig(config);
    }

    @PostMapping("/generate")
    public GenerateResult generate(@RequestBody(required = false) AppConfig config) {
        AppConfig currentConfig = config == null ? configService.readConfig() : config;
        return documentGenerationService.generate(currentConfig);
    }
}
